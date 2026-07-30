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
pub mod error;
pub mod handlers;
pub mod instance;
pub mod output;
pub mod server;
pub mod state;

use clap::Parser;
use cli::{Cli, Command};
use error::{CliError, Result};
use instance::Instance;
use output::{OutputFormat, resolve_format};
use std::process::ExitCode;

/// Environment variable for the log filter, checked before `RUST_LOG`.
const LOG_ENV: &str = "LAZYDAP_LOG";

/// Parse the command line, run it, and turn the outcome into an exit code.
pub async fn run_cli(args: Vec<String>) -> ExitCode {
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            // Help and version are successes that happen to print; everything
            // else clap rejects is a usage error, which is exit 2.
            return ExitCode::from(if error.use_stderr() { 2 } else { 0 });
        }
    };

    let format = resolve_format(cli.format);
    init_tracing(cli.command.as_ref().is_some_and(Command::is_daemon));

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
        // Bare `lazydap` becomes the TUI once there is one (M8+). Until then,
        // saying what exists beats an empty prompt.
        let _ = <Cli as clap::CommandFactory>::command().print_help();
        return Ok(());
    };

    let instance = Instance::resolve(cli.instance.as_deref())?;

    match command {
        Command::Daemon { .. } => server::run_daemon(instance).await,
        Command::Launch {
            program,
            stop_on_entry,
            cwd,
            adapter,
            args,
        } => {
            commands::launch(
                &instance,
                commands::LaunchOptions {
                    program,
                    args,
                    cwd,
                    adapter,
                    stop_on_entry,
                },
                format,
            )
            .await
        }
        Command::Status => commands::status(&instance, format).await,
        Command::Disconnect {
            session_id,
            no_terminate,
        } => commands::disconnect(&instance, session_id, !no_terminate, format).await,
        Command::Shutdown => commands::shutdown(&instance, format).await,
    }
}

/// Structured logging from the first thing `main` does (D015).
///
/// The daemon logs at `info` to stderr, which the client that spawned it
/// points at a log file. Subcommands are quiet by default: their stdout is
/// somebody's JSON pipeline, and their stderr is where errors go.
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

fn report(error: &CliError, format: OutputFormat) {
    match format {
        OutputFormat::Json => eprintln!(
            "{}",
            serde_json::to_string(&error.as_json())
                .unwrap_or_else(|_| format!(r#"{{"error":"{}"}}"#, error.label))
        ),
        OutputFormat::Table => eprintln!("error: {:#}", error.source),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("parse")
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
