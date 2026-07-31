//! The client half of each subcommand: talk to the daemon, print the answer.
//!
//! Every command renders both formats. JSON is a product feature with a stable
//! schema (`ARCHITECTURE.md`), so it is built explicitly here rather than
//! being whatever `serde` happened to derive for an internal type.
//!
//! Anything that depends on *where the user is* — resolving a relative path, a
//! `--timeout` default, reading `LAZYDAP_TIMEOUT` — is decided here rather than
//! in the daemon. The daemon's working directory is wherever it was started
//! and its environment is whatever it inherited, so a path or a default
//! resolved there would silently mean something else.

pub mod breakpoints;
pub mod diagnostics;
pub mod inspect;
pub mod launches;
pub mod session;
pub mod tui;
pub mod watch;

use crate::cli::WaitArgs;
use crate::client::DaemonClient;
use crate::error::{CliError, Result};
use lazydap_config::Config;
use lazydap_core::SessionId;
use lazydap_protocol::{ErrorCode, IpcError, Request, Response, StatusReport, WaitMode};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Environment variable for the default `--wait` timeout, in seconds.
const TIMEOUT_ENV: &str = "LAZYDAP_TIMEOUT";

/// The default `--wait` timeout when nothing says otherwise.
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

/// How much longer than the wait itself the client is prepared to sit there.
///
/// The daemon has to finish the wait, fetch a frame and write a reply; giving
/// up at the exact moment its own timer fires would turn a completed wait into
/// a client-side error.
const WAIT_SLACK: Duration = Duration::from_secs(15);

/// The `--wait` flags as the protocol wants them.
///
/// `--timeout 0` means "no timeout", which is the caller taking responsibility
/// for a program that may never stop. Anything else is seconds.
///
/// Three places can set the default, in the order of how specific they are:
/// the flag on this invocation, `LAZYDAP_TIMEOUT` in this shell, then
/// `[general] wait_timeout_seconds` in the user's config. The environment
/// beats the config deliberately — a variable set for one command is a more
/// specific statement than a file written once.
pub fn wait_mode(args: &WaitArgs, config: &Config) -> WaitMode {
    if !args.wait {
        return WaitMode::NoWait;
    }

    let seconds = args
        .timeout
        .or_else(|| {
            std::env::var(TIMEOUT_ENV)
                .ok()
                .and_then(|value| value.trim().parse().ok())
        })
        .or_else(|| config.wait_timeout_seconds())
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS);

    WaitMode::Wait {
        // Saturating rather than wrapping: a caller who writes
        // `--timeout 99999999999` should get a very long wait, not a very
        // short one.
        timeout_ms: Some(seconds.saturating_mul(1_000).try_into().unwrap_or(u32::MAX)),
    }
}

/// How long the client should be prepared to wait for the daemon's answer, or
/// `None` for as long as it takes.
pub fn client_timeout(wait: WaitMode) -> Option<Duration> {
    match wait {
        WaitMode::NoWait | WaitMode::Wait { timeout_ms: None } => Some(Duration::from_secs(60)),
        // No timeout on the daemon's side means none on ours either; the
        // caller asked to block until something happens.
        WaitMode::Wait {
            timeout_ms: Some(0),
        } => None,
        WaitMode::Wait {
            timeout_ms: Some(ms),
        } => Some(Duration::from_millis(ms as u64) + WAIT_SLACK),
    }
}

pub async fn fetch_status(client: &mut DaemonClient) -> Result<StatusReport> {
    match client.request(Request::Status).await? {
        Response::Status(report) => Ok(report),
        other => Err(unexpected(other)),
    }
}

/// Read a session id off the command line.
///
/// The protocol always carries an explicit id (D007); letting the user leave
/// it out is a client-side convenience, so parsing it is a client-side job.
/// Exit 2, not 1: this is a malformed argument, which the contract in
/// AGENTS.md calls a usage error. Nothing has been asked of the daemon yet, so
/// reporting it as a general failure would have a script retrying a command
/// that can never work.
pub fn parse_session_id(value: &str) -> Result<SessionId> {
    value
        .parse::<SessionId>()
        .map_err(|error| CliError::usage(format!("`{value}` is not a session id: {error}")))
}

/// The single active session, for commands that let you leave the id out.
pub async fn active_session_id(client: &mut DaemonClient) -> Result<SessionId> {
    fetch_status(client)
        .await?
        .session
        .map(|session| session.session_id)
        .ok_or_else(|| {
            CliError::from(IpcError::new(
                ErrorCode::SessionNotFound,
                "no active session; run `lazydap launch <program>` first",
            ))
        })
}

/// Turn a path the user typed into one the daemon and the adapter can agree
/// on.
///
/// Resolved here, against *this* process's working directory: the daemon's is
/// wherever it happened to be started, so `src/main.c` would otherwise mean a
/// different file — or, worse, a file that happens to exist there.
///
/// A path that does not resolve is refused rather than passed along. A
/// breakpoint in a file that is not there never verifies, and finding that out
/// as a silent `verified: false` twenty minutes later is a poor trade for the
/// error message here.
pub fn resolve_source(source: &Path) -> Result<PathBuf> {
    source.canonicalize().map_err(|error| {
        CliError::from(
            IpcError::new(
                ErrorCode::BadRequest,
                format!("cannot find {}: {error}", source.display()),
            )
            .with_details(serde_json::json!({ "source": source })),
        )
    })
}

pub fn unexpected(response: Response) -> CliError {
    CliError::general(anyhow::anyhow!(
        "the daemon answered with an unexpected response: {response:?}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(wait: bool, timeout: Option<u64>) -> WaitArgs {
        WaitArgs { wait, timeout }
    }

    fn mode(wait: bool, timeout: Option<u64>) -> WaitMode {
        wait_mode(&args(wait, timeout), &Config::default())
    }

    #[test]
    fn without_the_flag_nothing_waits() {
        assert_eq!(mode(false, None), WaitMode::NoWait);
    }

    #[test]
    fn a_timeout_is_given_in_seconds_and_sent_in_milliseconds() {
        assert_eq!(
            mode(true, Some(5)),
            WaitMode::Wait {
                timeout_ms: Some(5_000)
            },
        );
    }

    #[test]
    fn zero_seconds_is_the_documented_spelling_of_waiting_forever() {
        assert_eq!(
            mode(true, Some(0)),
            WaitMode::Wait {
                timeout_ms: Some(0)
            },
        );
    }

    #[test]
    fn an_absurd_timeout_saturates_rather_than_wrapping_round_to_a_short_one() {
        let saturated = mode(true, Some(u64::MAX));
        assert_eq!(
            saturated,
            WaitMode::Wait {
                timeout_ms: Some(u32::MAX)
            },
            "wrapping would turn `wait a very long time` into `wait no time at all`",
        );
    }

    #[test]
    fn the_client_outlasts_the_wait_it_asked_the_daemon_for() {
        // The daemon still has to fetch a frame and write a reply after its
        // own timer fires.
        let wait = WaitMode::Wait {
            timeout_ms: Some(30_000),
        };
        assert!(
            client_timeout(wait) > Some(Duration::from_secs(30)),
            "got: {:?}",
            client_timeout(wait),
        );
    }

    #[test]
    fn a_wait_with_no_timeout_does_not_get_one_from_the_client_either() {
        // Not "a very large one": `Instant + Duration` panics on overflow, so
        // a sentinel big enough to mean never is big enough to crash.
        let wait = WaitMode::Wait {
            timeout_ms: Some(0),
        };
        assert_eq!(client_timeout(wait), None);
    }

    #[test]
    fn the_config_file_sets_the_default_timeout_when_nothing_more_specific_does() {
        let path =
            std::env::temp_dir().join(format!("lazydap-timeout-{}.toml", std::process::id()));
        std::fs::write(&path, "[general]\nwait_timeout_seconds = 90\n").expect("write");
        let config = lazydap_config::load_config_from(&path).expect("load");
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            wait_mode(&args(true, None), &config),
            WaitMode::Wait {
                timeout_ms: Some(90_000)
            },
        );
        assert_eq!(
            wait_mode(&args(true, Some(5)), &config),
            WaitMode::Wait {
                timeout_ms: Some(5_000)
            },
            "the flag on this invocation is more specific than a file written once",
        );
    }

    #[test]
    fn a_malformed_session_id_is_a_usage_error_not_a_debugger_failure() {
        let error = match parse_session_id("not-a-uuid") {
            Err(error) => error,
            Ok(id) => unreachable!("that is not an id, got: {id}"),
        };
        assert_eq!(
            error.exit_code,
            crate::error::exit::USAGE,
            "exit 1 would have a script retrying a command that can never work",
        );
        assert_eq!(error.label, "UsageError", "got: {error}");
    }

    #[test]
    fn a_source_that_is_not_there_is_refused_with_the_path_quoted() {
        let error = match resolve_source(Path::new("/nowhere/at/all/main.c")) {
            Err(error) => error,
            Ok(path) => unreachable!("that file does not exist, got: {}", path.display()),
        };
        assert!(
            error.to_string().contains("/nowhere/at/all/main.c"),
            "got: {error}",
        );
    }
}
