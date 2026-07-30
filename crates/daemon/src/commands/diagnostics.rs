//! Version, doctor, logs, completions.
//!
//! Two of these never talk to a daemon. `lazydap version` printing a string
//! and `lazydap completions bash` printing a script are both things a shell
//! profile might run on every new terminal, and starting a background process
//! to do either would be a poor surprise.

use super::unexpected;
use crate::auto_spawn::ensure_daemon_running;
use crate::cli::Cli;
use crate::error::{CliError, Result};
use crate::instance::Instance;
use crate::output::{OutputFormat, Row, View};
use clap::CommandFactory;
use lazydap_protocol::{DoctorReport, LAZYDAP_PROTOCOL_VERSION, Request, Response};
use std::io::{Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

/// How often to look for new log lines while following.
const FOLLOW_INTERVAL: Duration = Duration::from_millis(200);

pub fn version(format: OutputFormat) -> Result<()> {
    let lazydap = env!("CARGO_PKG_VERSION");
    View::single(
        serde_json::json!({
            "lazydap": lazydap,
            "protocol": LAZYDAP_PROTOCOL_VERSION,
        }),
        format!("lazydap {lazydap} (protocol v{LAZYDAP_PROTOCOL_VERSION})"),
    )
    .print(format)
}

pub fn completions(shell: clap_complete::Shell) -> Result<()> {
    // Generated from the same command tree `--help` renders, so a subcommand
    // added in `cli.rs` is completable without anybody remembering to say so.
    let mut command = Cli::command();
    let name = command.get_name().to_string();
    clap_complete::generate(shell, &mut command, name, &mut std::io::stdout());
    Ok(())
}

pub async fn doctor(
    instance: &Instance,
    check_adapters: bool,
    check_state: bool,
    format: OutputFormat,
) -> Result<()> {
    // No flags means every check. Naming one narrows it, which is the only
    // reading of `--check-adapters` that is any use.
    let (check_adapters, check_state) = match (check_adapters, check_state) {
        (false, false) => (true, true),
        chosen => chosen,
    };

    let mut client = ensure_daemon_running(instance).await?;
    let response = client
        .request(Request::Doctor {
            check_adapters,
            check_state,
        })
        .await?;
    let Response::Doctor(report) = response else {
        return Err(unexpected(response));
    };

    let view = view(&report);
    view.print(format)?;

    // Everything goes to stdout (D025), including the verdict — but a failed
    // check is a failed command, so a script can branch on the exit code
    // rather than parsing the report.
    if !report.ok {
        return Err(CliError::general(anyhow::anyhow!(
            "{} check(s) failed",
            report.checks.iter().filter(|check| !check.ok).count(),
        )));
    }
    Ok(())
}

fn view(report: &DoctorReport) -> View {
    let rows = report
        .checks
        .iter()
        .map(|check| Row {
            id: check.name.clone(),
            cells: vec![
                check.name.clone(),
                if check.ok { "ok" } else { "FAILED" }.to_string(),
                check.detail.clone(),
            ],
            json: serde_json::to_value(check).unwrap_or(serde_json::Value::Null),
        })
        .collect();

    View::list(
        serde_json::json!({ "ok": report.ok, "checks": report.checks }),
        &["check", "status", "detail"],
        rows,
    )
}

/// Print the daemon's log.
///
/// Read straight off disk rather than through the daemon. The log is most
/// wanted precisely when the daemon is not answering, and a `lazydap logs`
/// that needed a healthy daemon would be useless exactly then.
pub async fn logs(
    instance: &Instance,
    limit: usize,
    level: Option<String>,
    follow: bool,
    purge: bool,
    format: OutputFormat,
) -> Result<()> {
    if purge {
        let existed = instance.log.exists();
        if existed {
            std::fs::remove_file(&instance.log).map_err(CliError::general)?;
        }
        return View::single(
            serde_json::json!({
                "log": instance.log,
                "purged": existed,
            }),
            if existed {
                format!("removed {}", instance.log.display())
            } else {
                "no log file to remove".to_string()
            },
        )
        .print(format);
    }

    if !instance.log.exists() {
        return View::list(serde_json::json!({ "lines": [] }), &["line"], Vec::new())
            .with_note(format!(
                "no log at {} yet; the daemon writes it when it starts",
                instance.log.display(),
            ))
            .print(format);
    }

    let contents = std::fs::read_to_string(&instance.log).map_err(CliError::general)?;
    let selected = tail(&contents, limit, level.as_deref());

    let rows = selected
        .iter()
        .map(|line| Row {
            id: line.clone(),
            cells: vec![line.clone()],
            json: serde_json::json!({ "line": line }),
        })
        .collect();

    View::list(serde_json::json!({ "lines": selected }), &["line"], rows).print(format)?;

    if follow {
        follow_log(&instance.log, level.as_deref()).await?;
    }
    Ok(())
}

/// The last `limit` lines that match `level`, oldest first.
fn tail(contents: &str, limit: usize, level: Option<&str>) -> Vec<String> {
    let matching: Vec<String> = contents
        .lines()
        .filter(|line| matches_level(line, level))
        .map(str::to_string)
        .collect();

    let skip = matching.len().saturating_sub(limit);
    matching.into_iter().skip(skip).collect()
}

/// Whether a log line is at the level asked for.
///
/// Matched on the line's text because the log is `tracing`'s human format, not
/// JSON. Crude, and honest about it: the level is the second field, and a
/// message that happens to contain the word is a false positive nobody is
/// harmed by.
fn matches_level(line: &str, level: Option<&str>) -> bool {
    match level {
        None => true,
        Some(level) => line
            .to_ascii_uppercase()
            .contains(&level.to_ascii_uppercase()),
    }
}

/// Print lines as the daemon appends them, until the caller gives up.
async fn follow_log(path: &Path, level: Option<&str>) -> Result<()> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path).map_err(CliError::general)?;
    let mut reader = BufReader::new(file);
    // Start at the end: everything before it has just been printed.
    reader
        .get_mut()
        .seek(SeekFrom::End(0))
        .map_err(CliError::general)?;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => tokio::time::sleep(FOLLOW_INTERVAL).await,
            Ok(_) => {
                let line = line.trim_end();
                if matches_level(line, level) {
                    println!("{line}");
                }
            }
            Err(error) => return Err(CliError::general(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "2026-07-30 INFO daemon.ipc: listening\n\
                       2026-07-30 WARN daemon.session: adapter died\n\
                       2026-07-30 INFO daemon.ipc: stopped\n";

    #[test]
    fn the_tail_is_the_newest_lines_in_the_order_they_were_written() {
        let lines = tail(LOG, 2, None);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("adapter died"), "got: {lines:?}");
        assert!(lines[1].contains("stopped"), "got: {lines:?}");
    }

    #[test]
    fn a_limit_larger_than_the_log_returns_the_whole_log() {
        assert_eq!(tail(LOG, 500, None).len(), 3);
    }

    #[test]
    fn a_level_filter_narrows_before_the_limit_is_applied() {
        // Otherwise `--level warn --limit 1` on a chatty log shows nothing:
        // the last line is an INFO and gets filtered away afterwards.
        let lines = tail(LOG, 1, Some("warn"));
        assert_eq!(lines.len(), 1, "got: {lines:?}");
        assert!(lines[0].contains("adapter died"), "got: {lines:?}");
    }

    #[test]
    fn the_level_filter_does_not_care_how_the_caller_capitalised_it() {
        assert!(matches_level("2026 WARN thing", Some("warn")));
        assert!(matches_level("2026 WARN thing", Some("WARN")));
        assert!(!matches_level("2026 INFO thing", Some("warn")));
    }
}
