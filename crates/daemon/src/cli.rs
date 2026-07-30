use crate::output::OutputFormat;
use clap::{Parser, Subcommand};
use lazydap_core::AdapterKind;
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

        /// Which debug adapter to use.
        #[arg(long, default_value = "codelldb")]
        adapter: AdapterKind,

        /// Arguments for the debuggee, after a `--` separator. They are kept
        /// separate so a debuggee flag can never be mistaken for a lazydap one.
        #[arg(last = true)]
        args: Vec<String>,
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
    },

    /// Stop the daemon and every session it owns.
    Shutdown,

    /// Run the daemon. Normally started automatically by the first command
    /// that needs it.
    Daemon {
        /// Stay in the terminal and log to stderr, for debugging.
        #[arg(long)]
        foreground: bool,
    },
}

impl Command {
    /// Whether this command runs the daemon itself rather than talking to one.
    pub fn is_daemon(&self) -> bool {
        matches!(self, Self::Daemon { .. })
    }
}
