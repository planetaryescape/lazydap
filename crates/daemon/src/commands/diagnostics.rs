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
use crate::output::{OutputFormat, Row, View, Wrote, print_line};
use clap::CommandFactory;
use lazydap_core::AdapterKind;
use lazydap_protocol::{DoctorCheck, DoctorReport, LAZYDAP_PROTOCOL_VERSION, Request, Response};
use std::io::{Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

/// How often to look for new log lines while following.
const FOLLOW_INTERVAL: Duration = Duration::from_millis(200);

/// The prefix every per-adapter `doctor` check's name carries.
const ADAPTER_CHECK_PREFIX: &str = "adapter.";

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
    // Generated into memory rather than straight at stdout: handed a
    // `std::io::stdout()`, `clap_complete` panics on the `EPIPE` that
    // `lazydap completions bash | head -20` produces. Going through
    // `print_line` instead makes a closed reader the end of the job, the way
    // it is for every other thing lazydap prints.
    let mut script = Vec::new();
    clap_complete::generate(shell, &mut command, name, &mut script);
    print_line(String::from_utf8_lossy(&script).trim_end())?;
    Ok(())
}

pub async fn doctor(
    instance: &Instance,
    check_adapters: bool,
    check_state: bool,
    format: OutputFormat,
) -> Result<()> {
    // No flags means every check, and only then is the daemon part of the
    // answer. Naming one narrows it to something this process can answer on
    // its own — which is the point of `--check-state`: the state file a daemon
    // refuses to start on is exactly the file you need this command to read.
    let everything = !check_adapters && !check_state;

    let mut checks = vec![config_check(instance)];
    if everything || check_adapters {
        checks.extend(adapter_checks(instance));
    }
    if everything || check_state {
        checks.push(state_check(instance));
    }
    if everything {
        checks.push(daemon_check(instance).await);
    }

    let report = DoctorReport {
        ok: verdict(&checks),
        checks,
    };

    let mut rendered = view(&report);
    if let Some(note) = note(&report) {
        rendered = rendered.with_note(note);
    }
    rendered.print(format)?;

    // Everything goes to stdout (D025), including the verdict — but a failed
    // check is a failed command, so a script can branch on the exit code
    // rather than parsing the report.
    if !report.ok {
        return Err(CliError::general(anyhow::anyhow!(
            "{}",
            failure_reason(&report.checks),
        )));
    }
    Ok(())
}

/// Whether `lazydap doctor` passes (D093).
///
/// `ok` means "lazydap can debug something here", not "this machine has every
/// adapter lazydap ships". The adapters are a menu of languages, and a Mac
/// with codelldb and no Go toolchain is a perfectly working install — but
/// `doctor` exited 1 there, which is the last line of the README, of
/// `install.sh` and of the Homebrew formula. Every other check still has to
/// pass, and losing the last adapter still fails: nothing can be debugged
/// then.
fn verdict(checks: &[DoctorCheck]) -> bool {
    let (adapters, rest): (Vec<&DoctorCheck>, Vec<&DoctorCheck>) =
        checks.iter().partition(|check| is_adapter(&check.name));

    rest.iter().all(|check| check.ok)
        && (adapters.is_empty() || adapters.iter().any(|check| check.ok))
}

fn is_adapter(name: &str) -> bool {
    name.starts_with(ADAPTER_CHECK_PREFIX)
}

/// Why the run failed, in the one line that goes to stderr.
///
/// "0 check(s) failed" is what counting the failed checks says when the only
/// thing wrong is that every adapter is missing — which is the one adapter
/// failure that does fail the run.
fn failure_reason(checks: &[DoctorCheck]) -> String {
    match checks
        .iter()
        .filter(|check| !check.ok && !is_adapter(&check.name))
        .count()
    {
        0 => "no usable debug adapter; install one of the ones listed above".to_string(),
        failed => format!("{failed} check(s) failed"),
    }
}

/// One check per adapter lazydap ships, whether or not this machine has it.
///
/// Resolved against *this* process's config and `PATH` (D050), which is what a
/// launch from this shell would use. Asking the daemon would answer for the
/// environment it was started in, days ago, from another directory.
fn adapter_checks(instance: &Instance) -> Vec<DoctorCheck> {
    AdapterKind::ALL
        .iter()
        .map(|&kind| {
            let name = format!("{ADAPTER_CHECK_PREFIX}{kind}");
            match crate::adapter::resolve_with(kind, &instance.config, None) {
                Ok(path) => DoctorCheck {
                    name,
                    ok: true,
                    detail: path.display().to_string(),
                },
                Err(error) => DoctorCheck {
                    name,
                    ok: false,
                    detail: detail_for(kind, error),
                },
            }
        })
        .collect()
}

/// Why an adapter is not usable, and — when the answer is "it is not here" —
/// how to get it.
///
/// The hint is only for a binary nobody could find. A pin at a path that is
/// not executable, or an interpreter without debugpy in it, is a *different*
/// problem, and both already say what to do; telling somebody to install
/// codelldb when they have one and mistyped its path sends them the wrong way.
fn detail_for(kind: AdapterKind, error: crate::adapter::AdapterError) -> String {
    match error {
        crate::adapter::AdapterError::NotFound { .. } => {
            format!("{error} — {}", install_hint(kind))
        }
        error => error.to_string(),
    }
}

/// Where to get an adapter this machine does not have.
///
/// Here rather than on `AdapterError`: it is advice for somebody reading
/// `doctor`, and a launch that fails for a missing adapter is not the moment
/// to be told how to install one.
fn install_hint(kind: AdapterKind) -> &'static str {
    match kind {
        AdapterKind::Codelldb => "install it from https://github.com/vadimcn/codelldb/releases",
        AdapterKind::Debugpy => "install it with `python3 -m pip install debugpy`",
        AdapterKind::Delve => {
            "install it with `go install github.com/go-delve/delve/cmd/dlv@latest`"
        }
    }
}

/// What `lazydap doctor` says about `.lazydap/state.toml`.
///
/// Read here rather than asked of the daemon. A state file the daemon refuses
/// to start on is precisely the case this check exists for, and routing it
/// through the daemon meant the one command that can name the broken line
/// could not run until somebody had already found it.
fn state_check(instance: &Instance) -> DoctorCheck {
    let name = "state.file".to_string();
    match lazydap_store::ProjectStore::load(&instance.project_root) {
        Ok(store) => DoctorCheck {
            name,
            ok: true,
            // A state file that does not exist yet is fine — most projects
            // never have one. What is worth reporting is where it would go.
            detail: format!(
                "{} ({})",
                store.path().display(),
                if store.path().exists() {
                    format!("{} breakpoints", store.breakpoints().len())
                } else {
                    "not created yet".to_string()
                },
            ),
        },
        Err(error) => DoctorCheck {
            name,
            ok: false,
            detail: one_line(&error.to_string()),
        },
    }
}

/// What the daemon says about itself, or why it could not be asked.
///
/// A daemon that will not start is a report line, not an aborted command:
/// `doctor` is what you run to find out why, and the checks above have usually
/// already named the reason.
async fn daemon_check(instance: &Instance) -> DoctorCheck {
    let name = "daemon".to_string();
    let failed = |detail: String| DoctorCheck {
        name: name.clone(),
        ok: false,
        detail: one_line(&detail),
    };

    // The socket path is added here rather than left to the error: an
    // auto-spawn failure can bottom out in a bare `Permission denied (os error
    // 13)`, and "denied on what" is the whole question a person runs `doctor`
    // to answer.
    let unreachable = |error: CliError| {
        failed(format!(
            "no daemon at {}: {:#}",
            instance.socket.display(),
            error.source,
        ))
    };

    let mut client = match ensure_daemon_running(instance).await {
        Ok(client) => client,
        Err(error) => return unreachable(error),
    };
    // Both checks are answered above, in the process whose config and `PATH`
    // the answers are about; what is left is the daemon describing itself.
    let request = Request::Doctor {
        check_adapters: false,
        check_state: false,
    };
    match client.request(request).await {
        Ok(Response::Doctor(report)) => report
            .checks
            .into_iter()
            .find(|check| check.name == name)
            .unwrap_or_else(|| failed("the daemon reported nothing about itself".to_string())),
        Ok(other) => failed(unexpected(other).to_string()),
        Err(error) => unreachable(error),
    }
}

/// What `lazydap doctor` says about the user's config file.
///
/// Three outcomes worth telling apart: there is one and it reads; there is one
/// and it does not, in which case the path *and* the parser's complaint are
/// the whole answer; or there is none, which is normal and is reported as the
/// path to create if you want one.
fn config_check(instance: &Instance) -> DoctorCheck {
    let name = "config.file".to_string();

    if let Some(problem) = &instance.config_problem {
        return DoctorCheck {
            name,
            ok: false,
            detail: one_line(problem),
        };
    }

    let detail = match lazydap_config::config_path() {
        Ok(path) if path.exists() => format!("{} (read)", path.display()),
        Ok(path) => format!("none; create {} to add one", path.display()),
        Err(error) => format!("no config directory on this machine: {error}"),
    };
    DoctorCheck {
        name,
        ok: true,
        detail,
    }
}

/// One line, so a multi-line parser error does not tear the table apart.
///
/// A TOML or JSON error is several lines with a caret diagram in it. The line
/// and column survive the flattening, which is the part worth having.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The line under the table: which adapters this machine does not have.
///
/// The table only. The JSON carries every check with its own `ok`, and a
/// second prose copy is one more thing to keep in step.
fn note(report: &DoctorReport) -> Option<String> {
    let missing: Vec<&str> = report
        .checks
        .iter()
        .filter(|check| !check.ok && is_adapter(&check.name))
        .map(|check| &check.name[ADAPTER_CHECK_PREFIX.len()..])
        .collect();

    (!missing.is_empty()).then(|| {
        format!(
            "not usable here: {}. lazydap does not need them all — each one adds the \
             languages it debugs.",
            missing.join(", "),
        )
    })
}

/// A missing adapter says lazydap cannot debug that language here, not that
/// lazydap is broken — and the verdict agrees, so the column must not say
/// FAILED next to an overall `ok`.
fn status(check: &DoctorCheck) -> &'static str {
    match (check.ok, is_adapter(&check.name)) {
        (true, _) => "ok",
        (false, true) => "missing",
        (false, false) => "FAILED",
    }
}

fn view(report: &DoctorReport) -> View {
    let rows = report
        .checks
        .iter()
        .map(|check| {
            Row::new(
                check.name.clone(),
                vec![
                    check.name.clone(),
                    status(check).to_string(),
                    check.detail.clone(),
                ],
                check,
            )
        })
        .collect();

    View::list(
        serde_json::json!({ "ok": report.ok, "checks": report.checks }),
        &["check", "status", "detail"],
        rows,
    )
}

#[cfg(test)]
mod config_check_tests {
    use super::*;
    use lazydap_config::Config;
    use std::path::PathBuf;

    fn instance(config_problem: Option<String>) -> Instance {
        Instance {
            name: "test".to_string(),
            project_root: PathBuf::from("/p"),
            socket: PathBuf::from("/tmp/s.sock"),
            lock: PathBuf::from("/tmp/s.lock"),
            pid: PathBuf::from("/tmp/s.pid"),
            log: PathBuf::from("/tmp/s.log"),
            config: Config::default(),
            config_problem,
        }
    }

    #[test]
    fn a_config_that_cannot_be_read_is_reported_rather_than_fatal() {
        // `doctor` is the command whose job is saying this. Failing to start
        // because of the very thing it was asked to diagnose would be absurd.
        let check = config_check(&instance(Some(
            "/home/me/.config/lazydap/config.toml is not valid lazydap config: \
             expected `=` at line 2"
                .to_string(),
        )));

        assert_eq!(check.name, "config.file");
        assert!(!check.ok);
        assert!(check.detail.contains("config.toml"), "the path is in it");
        assert!(check.detail.contains("line 2"), "and so is the reason");
    }

    #[test]
    fn a_machine_with_no_config_passes_and_says_where_to_put_one() {
        let check = config_check(&instance(None));
        assert!(check.ok, "not having one is the normal case: {check:?}");
        assert!(
            check.detail.contains("config.toml"),
            "got: {}",
            check.detail,
        );
    }
}

#[cfg(test)]
mod doctor_tests {
    use super::*;

    fn check(name: &str, ok: bool) -> DoctorCheck {
        DoctorCheck {
            name: name.to_string(),
            ok,
            detail: String::new(),
        }
    }

    #[test]
    fn one_missing_adapter_does_not_fail_the_verdict() {
        // D093. A Mac with codelldb and no Go toolchain is a working
        // lazydap, and `doctor` is the last line of the README, of install.sh
        // and of the Homebrew formula.
        assert!(verdict(&[
            check("config.file", true),
            check("adapter.codelldb", true),
            check("adapter.delve", false),
            check("daemon", true),
        ]));
    }

    #[test]
    fn losing_the_last_adapter_does_fail_it() {
        assert!(!verdict(&[
            check("config.file", true),
            check("adapter.codelldb", false),
            check("adapter.delve", false),
        ]));
    }

    #[test]
    fn anything_that_is_not_an_adapter_still_has_to_pass() {
        assert!(!verdict(&[
            check("config.file", false),
            check("adapter.codelldb", true),
        ]));
        assert!(!verdict(&[
            check("state.file", false),
            check("adapter.codelldb", true),
        ]));
    }

    #[test]
    fn a_run_with_no_adapter_checks_at_all_is_judged_on_the_rest() {
        // `--check-state` asks about one file and nothing else.
        assert!(verdict(&[
            check("config.file", true),
            check("state.file", true)
        ]));
    }

    #[test]
    fn losing_every_adapter_says_so_rather_than_counting_to_zero() {
        // The failed checks are all adapters, and adapters are not what the
        // count is about — so counting them gives "0 check(s) failed".
        let reason =
            failure_reason(&[check("config.file", true), check("adapter.codelldb", false)]);
        assert!(reason.contains("no usable debug adapter"), "got: {reason}");

        let reason = failure_reason(&[
            check("config.file", false),
            check("adapter.codelldb", false),
        ]);
        assert_eq!(reason, "1 check(s) failed");
    }

    #[test]
    fn a_missing_adapter_is_not_printed_as_a_failure() {
        // The column has to agree with the verdict, or the table says FAILED
        // under `ok: true`.
        assert_eq!(status(&check("adapter.delve", false)), "missing");
        assert_eq!(status(&check("adapter.delve", true)), "ok");
        assert_eq!(status(&check("config.file", false)), "FAILED");
    }

    #[test]
    fn the_note_names_the_adapters_this_machine_does_not_have() {
        let report = DoctorReport {
            ok: true,
            checks: vec![
                check("adapter.codelldb", true),
                check("adapter.debugpy", false),
                check("adapter.delve", false),
            ],
        };
        let note = note(&report).expect("a note");
        assert!(note.contains("debugpy, delve"), "got: {note}");
        assert!(!note.contains("codelldb"), "got: {note}");
    }

    #[test]
    fn a_machine_with_every_adapter_gets_no_note() {
        let report = DoctorReport {
            ok: true,
            checks: vec![check("adapter.codelldb", true)],
        };
        assert_eq!(note(&report), None);
    }

    #[test]
    fn a_parser_error_is_flattened_onto_one_line_keeping_the_position() {
        let flattened = one_line(
            "TOML parse error at line 2, column 1\n  |\n2 | [[breakpoints\n  | ^\ninvalid table header",
        );
        assert!(!flattened.contains('\n'), "got: {flattened}");
        assert!(flattened.contains("line 2, column 1"), "got: {flattened}");
    }
}

#[cfg(test)]
mod follow_tests {
    use super::*;

    #[test]
    fn a_stream_can_only_be_followed_in_a_format_that_has_no_end() {
        assert_eq!(
            FollowFormat::of(OutputFormat::Table).expect("plain"),
            FollowFormat::Plain
        );
        assert_eq!(
            FollowFormat::of(OutputFormat::Jsonl).expect("jsonl"),
            FollowFormat::Jsonl
        );

        for format in [OutputFormat::Json, OutputFormat::Csv, OutputFormat::Ids] {
            let error = match FollowFormat::of(format) {
                Err(error) => error,
                Ok(follow) => unreachable!("{format:?} cannot carry a stream, got: {follow:?}"),
            };
            assert_eq!(error.exit_code, crate::error::exit::USAGE);
            assert_eq!(error.as_json()["details"]["format"], format.as_str());
        }
    }

    #[test]
    fn a_followed_line_is_the_same_object_the_log_itself_prints() {
        // So `lazydap logs --format jsonl` and `lazydap logs --follow --format
        // jsonl` are one stream, not two shapes.
        assert_eq!(
            FollowFormat::Jsonl.render("2026-07-30 INFO daemon.ipc: listening"),
            r#"{"line":"2026-07-30 INFO daemon.ipc: listening"}"#,
        );
        assert_eq!(FollowFormat::Plain.render("a line"), "a line");
    }

    #[test]
    fn a_quote_in_a_log_line_does_not_break_the_object_around_it() {
        assert_eq!(
            FollowFormat::Jsonl.render(r#"said "hi""#),
            r#"{"line":"said \"hi\""}"#,
        );
    }
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
    // Decided before anything is printed: a `--follow` in a format that cannot
    // carry it must fail with an empty stdout, not with a document followed by
    // lines that do not belong to it.
    let follow = follow.then(|| FollowFormat::of(format)).transpose()?;

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

    let wrote = View::list(serde_json::json!({ "lines": selected }), &["line"], rows)
        .print_checked(format)?;

    match follow {
        // Nothing is reading any more, and waiting for a line to discover that
        // means waiting forever on a daemon that has gone quiet.
        _ if wrote == Wrote::ReaderGone => Ok(()),
        Some(follow) => follow_log(&instance.log, level.as_deref(), follow).await,
        None => Ok(()),
    }
}

/// How a followed log line is printed.
///
/// Only two formats can carry a stream. `--format json` is one object and
/// `--format csv` is a header with rows under it; both describe a result that
/// has finished, and a log that is still being written has not. `--format ids`
/// has no id to print. Rather than reject those late, `logs` refuses the
/// combination before it prints anything — it used to print the document and
/// then append bare log lines after it, which no parser survives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FollowFormat {
    /// The line as the daemon wrote it.
    Plain,
    /// `{"line": "..."}` — the object `lazydap logs` already prints per row,
    /// so following a log and reading one are the same shape.
    Jsonl,
}

impl FollowFormat {
    fn of(format: OutputFormat) -> Result<Self> {
        match format {
            OutputFormat::Table => Ok(Self::Plain),
            OutputFormat::Jsonl => Ok(Self::Jsonl),
            other => Err(CliError::usage_with_details(
                format!(
                    "`--follow` cannot print `--format {}`: a log that is still being written \
                     has no end to close a document at; use `--format jsonl` or \
                     `--format table`",
                    other.as_str(),
                ),
                serde_json::json!({ "format": other.as_str() }),
            )),
        }
    }

    fn render(self, line: &str) -> String {
        match self {
            Self::Plain => line.to_string(),
            Self::Jsonl => serde_json::json!({ "line": line }).to_string(),
        }
    }
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
///
/// A reader that goes away *while* this is waiting is only noticed on the next
/// line, because a pipe has nothing to say until somebody writes to it and
/// `poll`ing for the hangup would want libc. `tail -f` has the same property
/// for the same reason: `lazydap logs --follow | head -1` sits there until the
/// daemon logs something. A reader that is already gone when the first page is
/// printed *is* caught — see the `Wrote` check in [`logs`] — which is the case
/// that used to panic.
async fn follow_log(path: &Path, level: Option<&str>, format: FollowFormat) -> Result<()> {
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
                if matches_level(line, level)
                    && print_line(&format.render(line))? == Wrote::ReaderGone
                {
                    // `| head -1` closes the pipe on purpose. Following a log
                    // nobody is reading any more is the one way this loop ends
                    // without an error.
                    return Ok(());
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
