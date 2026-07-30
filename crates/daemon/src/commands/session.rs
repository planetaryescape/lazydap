//! Starting, moving and ending a debug session.

use super::{
    active_session_id, client_timeout, fetch_status, parse_session_id, unexpected, wait_mode,
};
use crate::auto_spawn::{ensure_daemon_running, shut_down_other_daemon};
use crate::cli::WaitArgs;
use crate::client::DaemonClient;
use crate::error::{CliError, Result};
use crate::instance::Instance;
use crate::output::{OutputFormat, View, or_dash, render_fields};
use lazydap_core::{AdapterKind, StepKind};
use lazydap_protocol::{
    ErrorCode, IpcError, LaunchRequest, Request, Response, StableState, StatusReport,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct LaunchOptions {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub adapter: AdapterKind,
    pub stop_on_entry: bool,
}

pub async fn launch(
    instance: &Instance,
    options: LaunchOptions,
    format: OutputFormat,
) -> Result<()> {
    // Resolve the working directory against *this* process, not the daemon's.
    // The daemon's own cwd is wherever it happened to be started, so a
    // relative `--cwd ../fixtures` would otherwise be resolved from the wrong
    // base — and silently, against a directory that may well exist.
    let cwd = match options.cwd {
        Some(cwd) => cwd.canonicalize().map_err(|source| {
            CliError::from(
                IpcError::new(
                    ErrorCode::InvalidLaunchConfig,
                    format!(
                        "cannot use {} as the working directory: {source}",
                        cwd.display()
                    ),
                )
                .with_details(serde_json::json!({ "cwd": cwd })),
            )
        })?,
        None => std::env::current_dir().map_err(CliError::general)?,
    };

    // Resolve the program here, not in the daemon: the daemon's working
    // directory is wherever it was started, and "./hello" would mean something
    // different there. Failing now also beats failing after spawning an
    // adapter.
    let program = cwd
        .join(&options.program)
        .canonicalize()
        .map_err(|source| {
            CliError::from(
                IpcError::new(
                    ErrorCode::InvalidLaunchConfig,
                    format!("cannot debug {}: {source}", options.program.display()),
                )
                .with_details(serde_json::json!({ "program": options.program })),
            )
        })?;

    // Resolved here, against this process's config and `PATH`, for the same
    // reason the program and the working directory are (D050). The daemon's
    // environment is whatever it inherited whenever it started, so a
    // `LAZYDAP_CONFIG_PATH` set for this command would mean nothing there.
    // Failing now also beats failing after a daemon has been spawned.
    let adapter_command = crate::adapter::discover_with(options.adapter, &instance.config)
        .map_err(|error| CliError::from(error.into_ipc()))?;

    let mut client = ensure_daemon_running(instance).await?;
    let response = client
        .request(Request::Launch(LaunchRequest {
            adapter: options.adapter,
            program,
            args: options.args,
            cwd,
            env: options.env,
            stop_on_entry: options.stop_on_entry,
            adapter_command: Some(adapter_command),
        }))
        .await?;

    let Response::Launched {
        session_id,
        state,
        reason,
        raw_reason,
        thread_id,
        capabilities,
        breakpoints,
    } = response
    else {
        return Err(unexpected(response));
    };

    let json = serde_json::json!({
        "session_id": session_id.to_string(),
        "state": state,
        "reason": reason,
        "raw_reason": raw_reason,
        "thread_id": thread_id,
        "capabilities": capabilities,
        "breakpoints": breakpoints,
    });

    View::single(
        json,
        render_fields(&[
            ("session_id", session_id.to_string()),
            ("state", state.as_str().to_string()),
            ("reason", or_dash(reason.as_ref())),
            ("thread_id", or_dash(thread_id)),
            ("breakpoints", breakpoints.len().to_string()),
        ]),
    )
    .print(format)
}

pub async fn status(instance: &Instance, format: OutputFormat) -> Result<()> {
    let mut client = ensure_daemon_running(instance).await?;
    let report = fetch_status(&mut client).await?;

    View::single(
        serde_json::to_value(&report).map_err(CliError::general)?,
        render_status(&report),
    )
    .print(format)
}

pub async fn disconnect(
    instance: &Instance,
    session_id: Option<String>,
    terminate: bool,
    dry_run: bool,
    format: OutputFormat,
) -> Result<()> {
    let mut client = ensure_daemon_running(instance).await?;

    let session_id = match session_id {
        Some(value) => parse_session_id(&value)?,
        None => active_session_id(&mut client).await?,
    };

    let response = client
        .request(Request::Disconnect {
            session_id,
            terminate,
            dry_run,
        })
        .await?;
    let Response::Disconnected {
        session_id,
        dry_run,
        terminated_debuggee,
    } = response
    else {
        return Err(unexpected(response));
    };

    View::single(
        serde_json::json!({
            "session_id": session_id.to_string(),
            "disconnected": !dry_run,
            "dry_run": dry_run,
            "terminated_debuggee": terminated_debuggee,
        }),
        if dry_run {
            format!(
                "would disconnect {session_id}{}",
                if terminated_debuggee {
                    ", killing the debuggee"
                } else {
                    ""
                },
            )
        } else {
            format!("disconnected {session_id}")
        },
    )
    .print(format)
}

/// Stop the daemon.
///
/// `--dry-run` is answered entirely from a `Status` call. The protocol's
/// `Shutdown` is a frozen unit variant — it is the escape hatch for talking to
/// a daemon whose version we do not speak, so it cannot carry flags — and a
/// preview mutates nothing, so it never needed to be one.
pub async fn shutdown(instance: &Instance, dry_run: bool, format: OutputFormat) -> Result<()> {
    // Deliberately not `ensure_daemon_running`: starting a daemon in order to
    // ask it to stop would be absurd.
    let mut client = match DaemonClient::connect(&instance.socket).await {
        Ok(client) => client,
        // A daemon from another build is still a daemon, and `lazydap
        // shutdown` promising to stop it and then leaving it running is the
        // worst of both. `Shutdown` crosses versions, so send it blind.
        Err(error) if error.label == "VersionMismatch" => {
            let peer_version = error
                .peer_protocol_version()
                .unwrap_or(lazydap_protocol::LAZYDAP_PROTOCOL_VERSION);
            if !dry_run {
                shut_down_other_daemon(&instance.socket, peer_version).await?;
            }
            return View::single(
                serde_json::json!({
                    "instance": instance.name,
                    "shutting_down": !dry_run,
                    "dry_run": dry_run,
                    "note": "the daemon was from another build",
                }),
                format!(
                    "daemon {} (another build) {}",
                    instance.name,
                    if dry_run {
                        "would be stopped"
                    } else {
                        "shutting down"
                    },
                ),
            )
            .print(format);
        }
        Err(_) => {
            return View::single(
                serde_json::json!({
                    "instance": instance.name,
                    "shutting_down": false,
                    "dry_run": dry_run,
                    "reason": "no daemon was running",
                }),
                "no daemon was running".to_string(),
            )
            .print(format);
        }
    };

    if dry_run {
        // The same question the real path answers, asked without acting on it.
        let report = fetch_status(&mut client).await?;
        let sessions: Vec<_> = report.session.into_iter().collect();
        return View::single(
            serde_json::json!({
                "instance": instance.name,
                "shutting_down": false,
                "dry_run": true,
                "sessions": sessions,
            }),
            format!(
                "would stop daemon {} (pid {}) and {} session(s)",
                instance.name,
                report.daemon_pid,
                sessions.len(),
            ),
        )
        .print(format);
    }

    let response = client.request(Request::Shutdown).await?;
    let Response::ShuttingDown { sessions } = response else {
        return Err(unexpected(response));
    };

    View::single(
        serde_json::json!({
            "instance": instance.name,
            "shutting_down": true,
            "dry_run": false,
            "sessions": sessions,
        }),
        format!("daemon {} shutting down", instance.name),
    )
    .print(format)
}

/// What a stepping command is asking the program to do.
#[derive(Debug, Clone, Copy)]
pub enum Movement {
    Continue { all_threads: bool },
    Step(StepKind),
    Pause,
}

/// Move the program, and print either the acknowledgement or the whole story.
pub async fn step(
    instance: &Instance,
    movement: Movement,
    thread: Option<i64>,
    wait: &WaitArgs,
    format: OutputFormat,
) -> Result<()> {
    let mut client = ensure_daemon_running(instance).await?;
    let session_id = active_session_id(&mut client).await?;
    let wait = wait_mode(wait, &instance.config);

    let request = match movement {
        Movement::Continue { all_threads } => Request::Continue {
            session_id,
            thread_id: thread,
            wait,
            all_threads,
        },
        Movement::Step(kind) => Request::Step {
            session_id,
            thread_id: thread,
            kind,
            wait,
        },
        Movement::Pause => Request::Pause {
            session_id,
            thread_id: thread,
            wait,
        },
    };

    match client.request_within(request, client_timeout(wait)).await? {
        Response::Stepped(blob) => print_stable_state(&blob, format),
        Response::Continued {
            session_id,
            thread_id,
        } => View::single(
            serde_json::json!({
                "session_id": session_id.to_string(),
                "thread_id": thread_id,
                "state": "running",
            }),
            format!("running (thread {})", or_dash(thread_id)),
        )
        .print(format),
        other => Err(unexpected(other)),
    }
}

/// Print a `--wait` blob.
///
/// The table is a summary; the JSON is the whole thing, because that is what
/// agents read and what `docs/blueprint/10-async-to-sync.md` specifies.
pub fn print_stable_state(blob: &StableState, format: OutputFormat) -> Result<()> {
    let json = serde_json::to_value(blob).map_err(CliError::general)?;

    let mut rows = vec![("state", blob.state.as_str().to_string())];
    if let Some(reason) = &blob.reason {
        rows.push(("reason", reason.to_string()));
    }
    if let Some(raw) = &blob.raw_reason {
        rows.push(("raw_reason", raw.clone()));
    }
    if let Some(thread_id) = blob.thread_id {
        rows.push(("thread_id", thread_id.to_string()));
    }
    if let Some(frame) = &blob.frame {
        rows.push(("frame", frame.to_string()));
    }
    if let Some(exit_code) = blob.exit_code {
        rows.push(("exit_code", exit_code.to_string()));
    }
    if !blob.hit_breakpoint_ids.is_empty() {
        rows.push(("breakpoint", join_ids(&blob.hit_breakpoint_ids)));
    }
    if !blob.additional_stopped_threads.is_empty() {
        rows.push((
            "also_stopped",
            blob.additional_stopped_threads
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    rows.push(("elapsed_ms", blob.elapsed_ms.to_string()));

    let mut table = render_fields(&rows);
    if !blob.captured_output.is_empty() {
        table.push_str("\n\noutput");
        for chunk in &blob.captured_output {
            table.push_str(&format!(
                "\n  [{}] {}",
                chunk.category,
                chunk.output.trim_end()
            ));
        }
        if blob.output_truncated {
            table.push_str("\n  ... (truncated)");
        }
    }

    View::single(json, table).print(format)
}

fn join_ids(ids: &[lazydap_core::BreakpointId]) -> String {
    ids.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_status(report: &StatusReport) -> String {
    let mut rows = vec![
        ("instance", report.instance.clone()),
        ("daemon_pid", report.daemon_pid.to_string()),
        ("uptime_ms", report.uptime_ms.to_string()),
        ("version", report.lazydap_version.clone()),
        ("protocol", report.protocol_version.to_string()),
    ];

    match &report.session {
        None => rows.push(("session", "-".to_string())),
        Some(session) => rows.extend([
            ("session_id", session.session_id.to_string()),
            ("adapter", session.adapter.to_string()),
            ("program", session.program.display().to_string()),
            ("state", session.state.as_str().to_string()),
            ("exit_code", or_dash(session.exit_code)),
            ("buffered_events", session.buffered_events.to_string()),
            ("output_chunks", session.captured_output_chunks.to_string()),
        ]),
    }

    render_fields(&rows)
}

/// `KEY=VALUE` pairs, as the debuggee's environment.
///
/// A pair with no `=` is refused rather than guessed at: `--env DEBUG` could
/// plausibly mean "pass DEBUG through from my shell" or "set DEBUG to empty",
/// and picking one silently would eventually be the wrong one.
pub fn parse_env(pairs: &[String]) -> Result<BTreeMap<String, String>> {
    pairs
        .iter()
        .map(|pair| {
            pair.split_once('=')
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .ok_or_else(|| CliError::usage(format!("`--env {pair}` is not a KEY=VALUE pair")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazydap_core::{
        BreakpointId, OutputCategory, OutputChunk, PauseReason, SessionId, SessionState,
        WaitOutcome,
    };
    use lazydap_protocol::SessionSummary;

    fn report(session: Option<SessionSummary>) -> StatusReport {
        StatusReport {
            instance: "lazydap-test".to_string(),
            daemon_pid: 4242,
            uptime_ms: 1200,
            protocol_version: 1,
            lazydap_version: "0.1.0".to_string(),
            session,
        }
    }

    #[test]
    fn a_daemon_with_no_session_says_so_rather_than_printing_nothing() {
        let rendered = render_status(&report(None));
        assert!(rendered.contains("session"), "got: {rendered}");
        assert!(rendered.contains("lazydap-test"), "got: {rendered}");
    }

    #[test]
    fn a_live_session_reports_its_state_and_buffered_output() {
        let rendered = render_status(&report(Some(SessionSummary {
            session_id: SessionId::new(),
            adapter: AdapterKind::Codelldb,
            program: PathBuf::from("/tmp/hello"),
            state: SessionState::Paused,
            exit_code: None,
            buffered_events: 7,
            captured_output_chunks: 3,
            dropped_events: 0,
            uptime_ms: 900,
        })));

        assert!(rendered.contains("paused"), "got: {rendered}");
        assert!(rendered.contains("/tmp/hello"), "got: {rendered}");
        assert!(
            rendered
                .lines()
                .any(|line| line.starts_with("output_chunks") && line.ends_with(" 3")),
            "got: {rendered}",
        );
    }

    #[test]
    fn an_environment_pair_without_a_value_is_a_usage_error_not_a_guess() {
        let error = match parse_env(&["DEBUG".to_string()]) {
            Err(error) => error,
            Ok(env) => unreachable!("that is not a pair, got: {env:?}"),
        };
        assert_eq!(error.exit_code, crate::error::exit::USAGE, "got: {error}");
    }

    #[test]
    fn an_environment_value_may_itself_contain_an_equals_sign() {
        let env = parse_env(&["ARGS=--flag=value".to_string()]).expect("parse");
        assert_eq!(env["ARGS"], "--flag=value");
    }

    #[test]
    fn a_pause_table_leads_with_the_state_and_shows_the_output_that_arrived() {
        let mut blob = StableState::new(WaitOutcome::Paused);
        blob.reason = Some(PauseReason::Breakpoint);
        blob.thread_id = Some(1);
        blob.hit_breakpoint_ids = vec![BreakpointId(2)];
        blob.captured_output = vec![OutputChunk::new(OutputCategory::Stdout, "hello\n")];
        blob.elapsed_ms = 42;

        // The rendering, not the printing: what matters is that the summary
        // names the things a person scanning it is looking for.
        let json = serde_json::to_value(&blob).expect("serialise");
        assert_eq!(json["state"], "paused");
        assert_eq!(json["hit_breakpoint_ids"][0], 2);
        assert_eq!(json["captured_output"][0]["output"], "hello\n");
    }

    #[test]
    fn breakpoint_ids_are_joined_the_way_a_person_reads_a_list() {
        assert_eq!(join_ids(&[BreakpointId(1), BreakpointId(4)]), "1, 4",);
    }
}
