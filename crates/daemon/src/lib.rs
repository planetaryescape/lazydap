//! `lazydap`: the CLI, and the daemon it drives.
//!
//! One binary (D002). Subcommands talk to a per-project daemon over a Unix
//! socket, starting one if there isn't one (D003); `lazydap daemon` is that
//! daemon. Everything lives in the library so integration tests can drive the
//! same code the binary does.

pub mod adapter;
pub mod auto_spawn;
pub mod cli;
pub mod client;
pub mod commands;
pub mod debuggee;
pub mod error;
pub mod handles;
pub mod handlers;
pub mod instance;
pub mod output;
pub mod server;
pub mod state;
pub mod wait;

use clap::Parser;
use cli::{Cli, Command};
use error::{CliError, Result};
use instance::Instance;
use lazydap_core::StepKind;
use lazydap_protocol::{ErrorCode, IpcError};
use output::{OutputFormat, resolve_format};
use std::process::ExitCode;

/// Environment variable for the log filter, checked before `RUST_LOG`.
const LOG_ENV: &str = "LAZYDAP_LOG";

/// Parse the command line, run it, and turn the outcome into an exit code.
pub async fn run_cli(args: Vec<String>) -> ExitCode {
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) => return report_usage_error(&error, &args),
    };

    let format = resolve_format(cli.format);
    // The TUI installs its own, pointed at the instance log file rather than at
    // the terminal it is about to take over — so nothing may claim the global
    // subscriber before it does. See `commands::tui::send_logs_to_the_file`.
    if !owns_the_terminal(cli.command.as_ref()) {
        init_tracing(cli.command.as_ref().is_some_and(Command::is_daemon));
    }

    match run(cli, format).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&error, format);
            ExitCode::from(error.exit_code)
        }
    }
}

async fn run(cli: Cli, format: OutputFormat) -> Result<()> {
    let Some(command) = cli.command else {
        // Bare `lazydap` is the TUI, but only for a person sitting at a
        // terminal. Anywhere else — a pipe, a CI job, `$(lazydap)` — saying
        // what exists beats taking over a terminal nobody is watching.
        if is_interactive() {
            let instance = Instance::resolve(cli.instance.as_deref())?;
            return commands::tui::run(&instance).await;
        }
        let _ = <Cli as clap::CommandFactory>::command().print_help();
        return Ok(());
    };

    // Two commands are pure output and must not start anything. Resolving an
    // instance is harmless, but `lazydap completions bash` in a shell profile
    // should not so much as look for a runtime directory.
    match command {
        Command::Version => return commands::diagnostics::version(format),
        Command::Completions { shell } => return commands::diagnostics::completions(shell),
        _ => {}
    }

    let instance = Instance::resolve(cli.instance.as_deref())?;
    check_config(&instance, &command, format)?;
    use commands::{breakpoints, diagnostics, inspect, launches, session, watch};

    match command {
        Command::Version | Command::Completions { .. } => unreachable!("handled above"),
        Command::Daemon { .. } => server::run_daemon(instance).await,
        Command::Tui => commands::tui::run(&instance).await,

        Command::Launch {
            program,
            stop_on_entry,
            cwd,
            env,
            adapter,
            args,
        } => {
            session::launch(
                &instance,
                session::LaunchOptions {
                    program,
                    args,
                    cwd,
                    env: session::parse_env(&env)?,
                    adapter,
                    // `lazydap launch` names a program, not an adapter binary;
                    // pinning one is the config file's job (D026) or a launch
                    // configuration's.
                    adapter_command: None,
                    stop_on_entry,
                },
                format,
            )
            .await
        }
        Command::Launches { command } => match command {
            cli::LaunchesCommand::List => launches::list(&instance, format).await,
            cli::LaunchesCommand::Run {
                name,
                stop_on_entry,
            } => launches::run(&instance, &name, stop_on_entry, format).await,
        },
        Command::Watch { command } => match command {
            cli::WatchCommand::Add {
                expression,
                label,
                dry_run,
            } => watch::add(&instance, expression, label, dry_run, format).await,
            cli::WatchCommand::List => watch::list(&instance, format).await,
            cli::WatchCommand::Remove {
                expression,
                ids,
                all,
                dry_run,
            } => watch::remove(&instance, expression, ids, all, dry_run, format).await,
        },
        Command::Status => session::status(&instance, format).await,
        Command::Disconnect {
            session_id,
            no_terminate,
            dry_run,
        } => session::disconnect(&instance, session_id, !no_terminate, dry_run, format).await,
        Command::Shutdown { dry_run } => session::shutdown(&instance, dry_run, format).await,

        Command::Continue {
            wait,
            all_threads,
            thread,
        } => {
            session::step(
                &instance,
                session::Movement::Continue { all_threads },
                thread,
                &wait,
                format,
            )
            .await
        }
        Command::Step { wait, thread } => {
            session::step(
                &instance,
                session::Movement::Step(StepKind::Over),
                thread,
                &wait,
                format,
            )
            .await
        }
        Command::StepIn { wait, thread } => {
            session::step(
                &instance,
                session::Movement::Step(StepKind::In),
                thread,
                &wait,
                format,
            )
            .await
        }
        Command::StepOut { wait, thread } => {
            session::step(
                &instance,
                session::Movement::Step(StepKind::Out),
                thread,
                &wait,
                format,
            )
            .await
        }
        Command::Pause { wait, thread } => {
            session::step(&instance, session::Movement::Pause, thread, &wait, format).await
        }

        Command::Break {
            location,
            list,
            remove,
            toggle,
            ids,
            all,
            condition,
            hit_condition,
            log_message,
            disabled,
            dry_run,
        } => {
            breakpoints::run(
                &instance,
                breakpoints::BreakArgs {
                    location,
                    list,
                    remove,
                    toggle,
                    ids,
                    all,
                    condition,
                    hit_condition,
                    log_message,
                    disabled,
                    dry_run,
                },
                format,
            )
            .await
        }

        Command::Stack {
            thread,
            levels,
            start,
        } => inspect::stack(&instance, thread, start, levels, format).await,
        Command::Scopes { frame } => inspect::scopes(&instance, frame, format).await,
        Command::Variables {
            reference,
            filter,
            start,
            count,
            max,
        } => inspect::variables(&instance, reference, filter, start, count, max, format).await,
        Command::Eval {
            expression,
            frame,
            context,
        } => inspect::eval(&instance, &expression, frame, context, format).await,
        Command::Threads => inspect::threads(&instance, format).await,
        Command::Output { since } => inspect::output(&instance, since, format).await,

        Command::Logs {
            limit,
            level,
            follow,
            purge,
        } => diagnostics::logs(&instance, limit, level, follow, purge, format).await,
        Command::Doctor {
            check_adapters,
            check_state,
        } => diagnostics::doctor(&instance, check_adapters, check_state, format).await,
    }
}

/// Refuse, or warn, when the user's config file could not be read.
///
/// Refuse for the commands that would act on it — launching with an adapter
/// chosen by a file we could not parse is not something to guess at. Warn for
/// everything else and carry on with the defaults: a typo in `config.toml`
/// must not take `shutdown`, `disconnect`, `status` or `logs` down with it,
/// because those are what you run when a debuggee is loose and something has
/// gone wrong.
///
/// The warning goes to stderr in table mode and to the log otherwise: a
/// caller parsing `--format json` gets its object on stdout unpolluted, and
/// `lazydap doctor` reports the same problem as a failed check either way.
fn check_config(instance: &Instance, command: &Command, format: OutputFormat) -> Result<()> {
    let Some(problem) = &instance.config_problem else {
        return Ok(());
    };

    if command.needs_config() {
        return Err(CliError::from(IpcError::new(
            ErrorCode::InvalidLaunchConfig,
            format!("{problem} — fix it, or run `lazydap doctor` to see where it is"),
        )));
    }

    // `doctor` reports this as a check of its own, on stdout, where the rest
    // of its answer is. Warning first would print the same TOML error twice,
    // the first time out of nowhere.
    if matches!(command, Command::Doctor { .. }) {
        return Ok(());
    }

    if format == OutputFormat::Table {
        eprintln!("warning: {problem}; carrying on with the defaults");
    } else {
        tracing::warn!(target: "cli.config", %problem, "carrying on with the defaults");
    }
    Ok(())
}

/// Whether there is a person at a terminal to hand the screen to.
///
/// **Both** streams, not just stdout. `echo "" | lazydap` leaves stdout on the
/// terminal and only stdin on a pipe — and stdin is the half the TUI needs,
/// because that is where keys come from. Checking stdout alone would take over
/// the terminal for a shell pipeline and then sit there unable to read a
/// keypress, including the one that quits.
fn is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// Report a command line clap refused, in the shape the caller can read.
///
/// clap writes its own errors, and they are good ones — for a person. A script
/// that asked for `--format json` gets JSON on stderr instead, in the same
/// shape as every other lazydap error, so one parser handles both "the daemon
/// said no" and "that is not a flag".
///
/// The flag is read out of the raw arguments because there is no parsed `Cli`
/// to consult: parsing is what failed. That leaves one gap — a caller who set
/// no `--format` and is on a terminal gets clap's human text, which is the
/// right answer for a person anyway.
fn report_usage_error(error: &clap::Error, args: &[String]) -> ExitCode {
    // Help and version are successes that happen to print, and they go to
    // stdout as themselves whatever the format.
    if !error.use_stderr() {
        let _ = error.print();
        return ExitCode::SUCCESS;
    }

    if !wants_machine_output(args) {
        let _ = error.print();
        return ExitCode::from(error::exit::USAGE);
    }

    // `render` gives the message without clap's ANSI colouring, which would
    // otherwise land in the middle of a JSON string.
    let message = error.render().to_string();
    let body = serde_json::json!({
        "error": "UsageError",
        "message": message.trim(),
        "details": { "kind": format!("{:?}", error.kind()) },
    });
    eprintln!(
        "{}",
        serde_json::to_string(&body).unwrap_or_else(|_| r#"{"error":"UsageError"}"#.to_string()),
    );
    ExitCode::from(error::exit::USAGE)
}

/// Whether the caller asked for machine-readable output, judged from the raw
/// arguments and where stdout is going.
fn wants_machine_output(args: &[String]) -> bool {
    use std::io::IsTerminal;

    let explicit = args.windows(2).any(|pair| {
        pair[0] == "--format" && matches!(pair[1].as_str(), "json" | "jsonl" | "csv" | "ids")
    }) || args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--format=json" | "--format=jsonl" | "--format=csv" | "--format=ids"
        )
    });

    explicit || !std::io::stdout().is_terminal()
}

/// Structured logging from the first thing `main` does (D015).
///
/// The daemon logs at `info` to stderr, which the client that spawned it
/// points at a log file. Subcommands are quiet by default: their stdout is
/// somebody's JSON pipeline, and their stderr is where errors go.
/// Whether this invocation is going to take the terminal over.
///
/// Bare `lazydap` on a tty is the TUI just as much as `lazydap tui` is, and it
/// is the same terminal either way — so both have to keep their logs off it.
fn owns_the_terminal(command: Option<&Command>) -> bool {
    match command {
        Some(Command::Tui) => true,
        None => is_interactive(),
        Some(_) => false,
    }
}

fn init_tracing(is_daemon: bool) {
    use std::io::IsTerminal;
    use tracing_subscriber::EnvFilter;

    let default = if is_daemon { "info" } else { "warn" };
    // Filter directives match tracing targets, and lazydap's targets are
    // dotted names (`daemon.ipc`, `dap.send`) rather than module paths — so
    // `LAZYDAP_LOG=daemon=debug` works and `LAZYDAP_LOG=lazydap_daemon=debug`
    // does not.
    //
    // `EnvFilter::new` panics on a malformed directive, which would mean a
    // typo in an environment variable takes the daemon down before it can log
    // anything about why. Fall back to the default instead.
    let filter = std::env::var(LOG_ENV)
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .and_then(|filter| EnvFilter::try_new(&filter).ok())
        .unwrap_or_else(|| EnvFilter::new(default));

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal());

    // A second call would fail; that only happens in tests, where the first
    // subscriber is the one we want anyway.
    let _ = subscriber.try_init();
}

/// Errors go to stderr in whichever dialect the caller reads.
///
/// Every machine format gets the JSON object: `--format csv` says how to print
/// a *result*, and an error is not one. What matters is that a caller parsing
/// anything at all gets something parseable rather than a sentence.
fn report(error: &CliError, format: OutputFormat) {
    match format {
        OutputFormat::Table => eprintln!("error: {:#}", error.source),
        _ => eprintln!(
            "{}",
            serde_json::to_string(&error.as_json())
                .unwrap_or_else(|_| format!(r#"{{"error":"{}"}}"#, error.label))
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("parse")
    }

    #[test]
    fn only_the_tui_keeps_its_logs_off_the_terminal() {
        // Every other command prints and exits, so stderr is where its warnings
        // belong. The TUI takes the terminal over, and a `warn` written to it
        // lands across the panes — which an out-of-scope watch would do on
        // every single step.
        assert!(owns_the_terminal(
            parse(&["lazydap", "tui"]).command.as_ref()
        ));

        for args in [
            &["lazydap", "status"][..],
            &["lazydap", "watch", "list"][..],
            &["lazydap", "daemon"][..],
        ] {
            assert!(
                !owns_the_terminal(parse(args).command.as_ref()),
                "{args:?} prints and exits; its logs belong on stderr",
            );
        }
    }

    #[test]
    fn debuggee_arguments_come_after_a_separator_so_flags_are_unambiguous() {
        let cli = parse(&[
            "lazydap",
            "launch",
            "./hello",
            "--stop-on-entry",
            "--",
            "--verbose",
        ]);

        match cli.command {
            Some(Command::Launch {
                program,
                stop_on_entry,
                args,
                ..
            }) => {
                assert_eq!(program.to_str(), Some("./hello"));
                assert!(stop_on_entry, "the flag before `--` belongs to lazydap");
                assert_eq!(args, vec!["--verbose"], "and the one after it does not");
            }
            other => unreachable!("expected a launch command, got: {other:?}"),
        }
    }

    #[test]
    fn the_instance_and_format_flags_work_before_or_after_the_subcommand() {
        let before = parse(&[
            "lazydap",
            "--instance",
            "demo",
            "--format",
            "json",
            "status",
        ]);
        let after = parse(&[
            "lazydap",
            "status",
            "--instance",
            "demo",
            "--format",
            "json",
        ]);

        assert_eq!(before.instance.as_deref(), Some("demo"));
        assert_eq!(after.instance.as_deref(), Some("demo"));
        assert_eq!(before.format, Some(OutputFormat::Json));
        assert_eq!(after.format, Some(OutputFormat::Json));
    }

    #[test]
    fn disconnect_terminates_the_debuggee_unless_told_otherwise() {
        match parse(&["lazydap", "disconnect"]).command {
            Some(Command::Disconnect { no_terminate, .. }) => {
                assert!(!no_terminate, "killing the debuggee is the default");
            }
            other => unreachable!("expected a disconnect command, got: {other:?}"),
        }
    }

    #[test]
    fn an_unknown_subcommand_is_a_usage_error() {
        let error = Cli::try_parse_from(["lazydap", "explode"]).expect_err("no such command");
        assert!(error.use_stderr(), "usage errors exit 2, not 0");
    }
}
