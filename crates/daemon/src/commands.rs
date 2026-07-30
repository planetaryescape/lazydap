//! The client half of each subcommand: talk to the daemon, print the answer.
//!
//! Every command renders both formats. JSON is a product feature with a stable
//! schema (`ARCHITECTURE.md`), so it is built explicitly here rather than
//! being whatever `serde` happened to derive for an internal type.

use crate::auto_spawn::ensure_daemon_running;
use crate::client::DaemonClient;
use crate::error::{CliError, Result};
use crate::instance::Instance;
use crate::output::{OutputFormat, print_json, render_fields};
use lazydap_core::{AdapterKind, SessionId};
use lazydap_protocol::{ErrorCode, IpcError, LaunchRequest, Request, Response, StatusReport};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct LaunchOptions {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub adapter: AdapterKind,
    pub stop_on_entry: bool,
}

pub async fn launch(
    instance: &Instance,
    options: LaunchOptions,
    format: OutputFormat,
) -> Result<()> {
    let cwd = match options.cwd {
        Some(cwd) => cwd,
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

    let mut client = ensure_daemon_running(instance).await?;
    let response = client
        .request(Request::Launch(LaunchRequest {
            adapter: options.adapter,
            program,
            args: options.args,
            cwd,
            env: BTreeMap::new(),
            stop_on_entry: options.stop_on_entry,
        }))
        .await?;

    let Response::Launched {
        session_id,
        state,
        reason,
        thread_id,
        capabilities,
    } = response
    else {
        return Err(unexpected(response));
    };

    let json = serde_json::json!({
        "session_id": session_id.to_string(),
        "state": state,
        "reason": reason,
        "thread_id": thread_id,
        "capabilities": capabilities,
    });

    match format {
        OutputFormat::Json => print_json(&json)?,
        OutputFormat::Table => println!(
            "{}",
            render_fields(&[
                ("session_id", session_id.to_string()),
                ("state", state.as_str().to_string()),
                (
                    "reason",
                    reason
                        .map(|reason| reason.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "thread_id",
                    thread_id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ),
            ])
        ),
    }
    Ok(())
}

pub async fn status(instance: &Instance, format: OutputFormat) -> Result<()> {
    let mut client = ensure_daemon_running(instance).await?;
    let report = fetch_status(&mut client).await?;

    match format {
        OutputFormat::Json => {
            print_json(&serde_json::to_value(&report).map_err(CliError::general)?)?
        }
        OutputFormat::Table => println!("{}", render_status(&report)),
    }
    Ok(())
}

pub async fn disconnect(
    instance: &Instance,
    session_id: Option<String>,
    terminate: bool,
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
        })
        .await?;
    let Response::Disconnected { session_id } = response else {
        return Err(unexpected(response));
    };

    match format {
        OutputFormat::Json => print_json(&serde_json::json!({
            "session_id": session_id.to_string(),
            "disconnected": true,
            "terminated_debuggee": terminate,
        }))?,
        OutputFormat::Table => println!("disconnected {session_id}"),
    }
    Ok(())
}

pub async fn shutdown(instance: &Instance, format: OutputFormat) -> Result<()> {
    // Deliberately not `ensure_daemon_running`: starting a daemon in order to
    // ask it to stop would be absurd.
    let mut client = match DaemonClient::connect(&instance.socket).await {
        Ok(client) => client,
        Err(_) => {
            match format {
                OutputFormat::Json => print_json(&serde_json::json!({
                    "instance": instance.name,
                    "shutting_down": false,
                    "reason": "no daemon was running",
                }))?,
                OutputFormat::Table => println!("no daemon was running"),
            }
            return Ok(());
        }
    };

    let response = client.request(Request::Shutdown).await?;
    if response != Response::ShuttingDown {
        return Err(unexpected(response));
    }

    match format {
        OutputFormat::Json => print_json(&serde_json::json!({
            "instance": instance.name,
            "shutting_down": true,
        }))?,
        OutputFormat::Table => println!("daemon {} shutting down", instance.name),
    }
    Ok(())
}

async fn fetch_status(client: &mut DaemonClient) -> Result<StatusReport> {
    match client.request(Request::Status).await? {
        Response::Status(report) => Ok(report),
        other => Err(unexpected(other)),
    }
}

/// Read a session id off the command line.
///
/// The protocol always carries an explicit id (D007); letting the user leave
/// it out is a client-side convenience, so parsing it is a client-side job.
fn parse_session_id(value: &str) -> Result<SessionId> {
    value.parse::<SessionId>().map_err(|error| {
        CliError::from(IpcError::new(
            ErrorCode::BadRequest,
            format!("`{value}` is not a session id: {error}"),
        ))
    })
}

/// The single active session, for commands that let you leave the id out.
async fn active_session_id(client: &mut DaemonClient) -> Result<SessionId> {
    fetch_status(client)
        .await?
        .session
        .map(|session| session.session_id)
        .ok_or_else(|| {
            CliError::from(IpcError::new(
                ErrorCode::SessionNotFound,
                "no active session",
            ))
        })
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
            (
                "exit_code",
                session
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            ),
            ("buffered_events", session.buffered_events.to_string()),
            ("output_chunks", session.captured_output_chunks.to_string()),
        ]),
    }

    render_fields(&rows)
}

fn unexpected(response: Response) -> CliError {
    CliError::general(anyhow::anyhow!(
        "the daemon answered with an unexpected response: {response:?}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazydap_core::SessionState;
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
    fn a_malformed_session_id_is_rejected_before_the_daemon_is_bothered() {
        let error = match parse_session_id("not-a-uuid") {
            Err(error) => error,
            Ok(id) => unreachable!("that is not an id, got: {id}"),
        };
        assert_eq!(error.label, "BadRequest", "got: {error}");
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
}
