use crate::output::OutputFormat;
use clap::{Parser, Subcommand};
use lazydap_core::{AdapterKind, EvalContext, VariableFilter};
use std::path::PathBuf;

/// A scriptable, terminal-first debugger.
#[derive(Debug, Parser)]
#[command(
    name = "lazydap",
    version,
    about = "A scriptable, terminal-first debugger",
    long_about = None,
)]
pub struct Cli {
    /// Which daemon to talk to. Defaults to one per project root, and can also
    /// be set with LAZYDAP_INSTANCE.
    #[arg(long, global = true)]
    pub instance: Option<String>,

    /// Output format. Defaults to `table` on a terminal and `json` when piped.
    #[arg(long, global = true, value_enum)]
    pub format: Option<OutputFormat>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start a program under the debugger.
    Launch {
        /// The program to debug.
        program: PathBuf,

        /// Stop at the program's entry point instead of running to the first
        /// breakpoint.
        #[arg(long)]
        stop_on_entry: bool,

        /// Working directory for the debuggee. Defaults to the current one.
        #[arg(long)]
        cwd: Option<PathBuf>,

        /// Environment for the debuggee, as KEY=VALUE. Repeatable.
        #[arg(long = "env", value_name = "KEY=VALUE")]
        env: Vec<String>,

        /// Which debug adapter to use. Defaults to the one the program's file
        /// extension implies — debugpy for `.py`, codelldb otherwise.
        #[arg(long)]
        adapter: Option<AdapterKind>,

        /// Arguments for the debuggee, after a `--` separator. They are kept
        /// separate so a debuggee flag can never be mistaken for a lazydap one.
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// List the project's named launch configurations, or run one.
    ///
    /// They come from `.lazydap/state.toml` and from `.vscode/launch.json`,
    /// which lazydap reads and never writes.
    Launches {
        #[command(subcommand)]
        command: LaunchesCommand,
    },

    /// Show the daemon and its current session.
    Status,

    /// End the current session.
    Disconnect {
        /// Which session to end. Defaults to the active one.
        #[arg(long)]
        session_id: Option<String>,

        /// Leave the debuggee running instead of killing it.
        #[arg(long)]
        no_terminate: bool,

        /// Report what would be ended, and end nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Stop the daemon and every session it owns.
    Shutdown {
        /// Report what would be stopped, and stop nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Resume the program.
    #[command(visible_alias = "c")]
    Continue {
        #[command(flatten)]
        wait: WaitArgs,

        /// Wait for every thread to stop, not just the first.
        #[arg(long)]
        all_threads: bool,

        /// Which thread to resume. Defaults to the one that stopped last.
        #[arg(long)]
        thread: Option<i64>,
    },

    /// Run the next line, stepping over any call in it.
    #[command(visible_alias = "next")]
    Step {
        #[command(flatten)]
        wait: WaitArgs,
        #[arg(long)]
        thread: Option<i64>,
    },

    /// Step into the call on this line.
    #[command(name = "step-in", visible_alias = "step-into")]
    StepIn {
        #[command(flatten)]
        wait: WaitArgs,
        #[arg(long)]
        thread: Option<i64>,
    },

    /// Run until the current function returns.
    #[command(name = "step-out")]
    StepOut {
        #[command(flatten)]
        wait: WaitArgs,
        #[arg(long)]
        thread: Option<i64>,
    },

    /// Interrupt a running program.
    Pause {
        #[command(flatten)]
        wait: WaitArgs,
        #[arg(long)]
        thread: Option<i64>,
    },

    /// Set, list, remove or toggle breakpoints.
    ///
    /// Breakpoints are project state: they are remembered in
    /// `.lazydap/state.toml` and applied to every session you launch, whether
    /// or not one is running when you set them.
    #[command(name = "break", visible_alias = "b")]
    Break {
        /// Where to break, as `file:line`.
        #[arg(value_name = "FILE:LINE")]
        location: Option<String>,

        /// List every breakpoint in the project.
        #[arg(long, conflicts_with_all = ["remove", "toggle"])]
        list: bool,

        /// Remove the selected breakpoints.
        #[arg(long, conflicts_with = "toggle")]
        remove: bool,

        /// Enable or disable the selected breakpoints.
        #[arg(long)]
        toggle: bool,

        /// Select by id. Repeatable, and what `--format ids` output feeds.
        #[arg(long = "id", value_name = "ID")]
        ids: Vec<u32>,

        /// Select every breakpoint in the project.
        #[arg(long)]
        all: bool,

        /// Only break when this expression is true.
        #[arg(long)]
        condition: Option<String>,

        /// Only break once the hit count matches, e.g. `>= 10`.
        #[arg(long)]
        hit_condition: Option<String>,

        /// Log this message instead of pausing. Braces interpolate:
        /// `--log "x = {x}"`.
        #[arg(long = "log", value_name = "MESSAGE")]
        log_message: Option<String>,

        /// Record it, but leave it switched off.
        #[arg(long)]
        disabled: bool,

        /// Report what would change, and change nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Show the call stack of a paused program.
    Stack {
        /// Which thread. Defaults to the one that stopped last.
        #[arg(long)]
        thread: Option<i64>,

        /// How many frames. Defaults to all of them.
        #[arg(long)]
        levels: Option<u32>,

        /// Skip this many frames from the top.
        #[arg(long)]
        start: Option<u32>,
    },

    /// Show the variable scopes of a frame.
    Scopes {
        /// Which frame. Defaults to the top one.
        #[arg(long)]
        frame: Option<i64>,
    },

    /// Expand a scope or a structured variable.
    Variables {
        /// The `variables_reference` from `scopes` or a parent variable.
        #[arg(long)]
        reference: i64,

        /// Fetch only named members or only indexed elements.
        #[arg(long, default_value = "all")]
        filter: VariableFilter,

        /// Skip this many.
        #[arg(long)]
        start: Option<u32>,

        /// Take at most this many.
        #[arg(long)]
        count: Option<u32>,
    },

    /// Evaluate an expression in the debuggee.
    Eval {
        /// The expression, in the debuggee's own language.
        expression: String,

        /// Which frame to evaluate in. Defaults to the top one.
        #[arg(long)]
        frame: Option<i64>,

        /// How the adapter should read the expression. `watch` and `hover`
        /// evaluate it in the program; `repl` runs it as an adapter command,
        /// which for codelldb means an LLDB command.
        #[arg(long, default_value = "watch")]
        context: EvalContext,
    },

    /// List the debuggee's threads.
    Threads,

    /// Show output the debuggee has produced.
    Output {
        /// Only output at or after this Unix-epoch millisecond.
        #[arg(long)]
        since: Option<u64>,
    },

    /// Show the daemon's log.
    Logs {
        /// Show at most this many lines, from the end.
        #[arg(long, default_value_t = 200)]
        limit: usize,

        /// Only lines at this level or louder.
        #[arg(long)]
        level: Option<String>,

        /// Keep printing as the daemon writes more.
        #[arg(long)]
        follow: bool,

        /// Delete the log file instead of printing it.
        #[arg(long, conflicts_with_all = ["follow", "limit"])]
        purge: bool,
    },

    /// Check that everything lazydap needs is where it should be.
    Doctor {
        /// Only check the adapters.
        #[arg(long)]
        check_adapters: bool,

        /// Only check the project state file.
        #[arg(long)]
        check_state: bool,
    },

    /// Print the lazydap and protocol versions.
    Version,

    /// Print a shell completion script.
    Completions {
        /// Which shell.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Open the terminal UI. This is also what bare `lazydap` does on a
    /// terminal.
    Tui,

    /// Run the daemon. Normally started automatically by the first command
    /// that needs it.
    Daemon {
        /// Stay in the terminal and log to stderr, for debugging.
        #[arg(long)]
        foreground: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum LaunchesCommand {
    /// Show every launch configuration, and whether lazydap can run it.
    List,

    /// Start the configuration with this name.
    Run {
        /// Its `name` in `launch.json` or `state.toml`.
        name: String,

        /// Stop at the program's entry point, whatever the configuration says.
        #[arg(long)]
        stop_on_entry: bool,
    },
}

/// The `--wait` pair, shared by every command that moves the program.
///
/// Flattened rather than repeated so the flags cannot drift apart between
/// `continue` and `step`, and so `--help` describes them identically.
#[derive(Debug, Clone, clap::Args)]
pub struct WaitArgs {
    /// Block until the program pauses, exits or is terminated, and return one
    /// JSON object describing everything that happened. Always use this from a
    /// script or an agent.
    #[arg(long)]
    pub wait: bool,

    /// How long to wait, in seconds. `0` waits forever. Defaults to 30, or to
    /// LAZYDAP_TIMEOUT.
    #[arg(long, requires = "wait")]
    pub timeout: Option<u64>,
}

impl Command {
    /// Whether this command runs the daemon itself rather than talking to one.
    pub fn is_daemon(&self) -> bool {
        matches!(self, Self::Daemon { .. })
    }

    /// Whether this command would *act* on the user's config file.
    ///
    /// Only the launch-class commands do: the adapter binary comes from it,
    /// and starting a debugger with the wrong one is worse than not starting.
    /// Everything else — `status`, `shutdown`, `disconnect`, `logs`, stepping,
    /// inspection — either uses a default it can state, or does not read the
    /// config at all, and must keep working when the file has a typo in it.
    /// Those are the commands you reach for *because* something is wrong.
    ///
    /// `doctor` is deliberately not here. Its job is to report the problem,
    /// which it cannot do if the problem stops it running.
    pub fn needs_config(&self) -> bool {
        match self {
            Self::Launch { .. } => true,
            Self::Launches { command } => matches!(command, LaunchesCommand::Run { .. }),
            _ => false,
        }
    }

    /// Whether this command can answer without a daemon at all.
    ///
    /// `version` and `completions` are pure output. Starting a background
    /// process to print a version string would be absurd, and would make
    /// `lazydap completions bash` in a shell profile spawn a daemon on every
    /// new terminal.
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Version | Self::Completions { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_tree_is_internally_consistent() {
        // clap's own audit: duplicate flags, a `requires` naming an argument
        // that does not exist, conflicting defaults. Cheap, and it fails at
        // the exact line that broke rather than at runtime.
        Cli::command().debug_assert();
    }

    #[test]
    fn a_timeout_without_a_wait_is_a_usage_error() {
        // `--timeout` on a fire-and-forget command means nothing, and quietly
        // ignoring it would leave a caller believing they had set one.
        let error = Cli::try_parse_from(["lazydap", "continue", "--timeout", "5"])
            .expect_err("--timeout requires --wait");
        assert!(error.use_stderr(), "got: {error}");
    }

    #[test]
    fn step_answers_to_the_name_half_the_world_learned_from_gdb() {
        for spelling in ["step", "next"] {
            let cli = Cli::try_parse_from(["lazydap", spelling]).expect("parse");
            assert!(
                matches!(cli.command, Some(Command::Step { .. })),
                "{spelling}"
            );
        }
    }

    #[test]
    fn step_in_accepts_both_spellings_the_ecosystem_uses() {
        for spelling in ["step-in", "step-into"] {
            let cli = Cli::try_parse_from(["lazydap", spelling]).expect("parse");
            assert!(
                matches!(cli.command, Some(Command::StepIn { .. })),
                "{spelling}",
            );
        }
    }

    #[test]
    fn breaking_and_listing_are_the_same_subcommand() {
        let cli = Cli::try_parse_from(["lazydap", "break", "main.c:19"]).expect("parse");
        match cli.command {
            Some(Command::Break { location, list, .. }) => {
                assert_eq!(location.as_deref(), Some("main.c:19"));
                assert!(!list);
            }
            other => unreachable!("expected a break command, got: {other:?}"),
        }
    }

    #[test]
    fn listing_and_removing_cannot_be_asked_for_at_once() {
        Cli::try_parse_from(["lazydap", "break", "--list", "--remove", "--all"])
            .expect_err("one mode at a time");
    }

    #[test]
    fn ids_are_repeatable_so_a_pipeline_can_pass_several() {
        let cli = Cli::try_parse_from(["lazydap", "break", "--remove", "--id", "1", "--id", "2"])
            .expect("parse");
        match cli.command {
            Some(Command::Break { ids, remove, .. }) => {
                assert_eq!(ids, vec![1, 2]);
                assert!(remove);
            }
            other => unreachable!("expected a break command, got: {other:?}"),
        }
    }

    #[test]
    fn the_commands_you_reach_for_when_things_go_wrong_do_not_need_the_config() {
        // A typo in config.toml must not take down the tools you use to
        // recover from a debuggee that is still running.
        for command in [
            Command::Status,
            Command::Shutdown { dry_run: false },
            Command::Disconnect {
                session_id: None,
                no_terminate: false,
                dry_run: false,
            },
            Command::Doctor {
                check_adapters: false,
                check_state: false,
            },
            Command::Threads,
        ] {
            assert!(!command.needs_config(), "got: {command:?}");
        }
    }

    #[test]
    fn launching_needs_the_config_because_the_adapter_comes_from_it() {
        let launch = Cli::try_parse_from(["lazydap", "launch", "./app"]).expect("parse");
        assert!(launch.command.expect("a command").needs_config());

        let run = Cli::try_parse_from(["lazydap", "launches", "run", "Debug"]).expect("parse");
        assert!(run.command.expect("a command").needs_config());

        // Listing does not launch anything, so a broken config costs it
        // nothing it cannot state.
        let list = Cli::try_parse_from(["lazydap", "launches", "list"]).expect("parse");
        assert!(!list.command.expect("a command").needs_config());
    }

    #[test]
    fn version_and_completions_do_not_need_a_daemon() {
        assert!(Command::Version.is_local());
        assert!(
            Command::Completions {
                shell: clap_complete::Shell::Bash,
            }
            .is_local(),
            "a shell profile must not spawn a daemon per terminal",
        );
        assert!(!Command::Status.is_local());
    }
}
