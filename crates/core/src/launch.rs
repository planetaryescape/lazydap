//! Named ways to start a program under the debugger.
//!
//! Two files describe these, and lazydap owns neither of them completely:
//! `.lazydap/state.toml` is lazydap's own (`lazydap-store` reads it), and
//! `.vscode/launch.json` belongs to VS Code and is read-only here
//! (`lazydap-config` imports it, D008). Both end up as this type, so the
//! command that runs one does not care which file it came from.
//!
//! Deliberately permissive about what it can hold. A configuration naming an
//! adapter lazydap does not ship, or asking to attach to a running process, is
//! still worth *listing* — telling someone their `launch.json` has four
//! configurations and lazydap can run one of them is far more useful than
//! showing one and silently dropping three.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::AdapterKind;

/// One named way to start (or attach to) a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchConfig {
    pub name: String,
    /// Which adapter this needs, when it is one lazydap has.
    ///
    /// `None` means the configuration named something else — `"python"`,
    /// `"go"` — which is not an error in the file, only a thing v0.1 cannot
    /// run (D013).
    pub adapter: Option<AdapterKind>,
    /// What the file called it, kept whether or not it mapped to an adapter.
    /// A person reading `lazydap launches list` wants to see `python` there,
    /// not a blank.
    pub adapter_type: String,
    pub kind: LaunchKind,
    pub program: Option<PathBuf>,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub stop_on_entry: bool,
    pub source: LaunchConfigSource,
    /// `${...}` variables in this configuration that nothing could expand,
    /// left in the strings exactly as they were written.
    ///
    /// Substituting an empty string for `${command:pickProcess}` would turn a
    /// configuration lazydap cannot honour into one that looks fine and starts
    /// the wrong thing.
    pub unresolved: Vec<String>,
    /// A problem found while *reading* this configuration that makes it
    /// unrunnable.
    ///
    /// Recorded at import rather than recomputed from the fields, because the
    /// fields no longer show it: an argument string with an unterminated quote
    /// leaves no arguments behind to look wrong. Listing the configuration
    /// with the reason beats dropping it, which would have the caller hunting
    /// for a configuration they can see in their editor.
    pub blocked: Option<NotRunnable>,
}

/// Which file a configuration came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LaunchConfigSource {
    /// `.lazydap/state.toml`, lazydap's own.
    ProjectState,
    /// `.vscode/launch.json`, inherited (D008).
    VsCodeLaunchJson,
}

impl LaunchConfigSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProjectState => "state.toml",
            Self::VsCodeLaunchJson => "launch.json",
        }
    }
}

impl std::fmt::Display for LaunchConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Start a new process, or join one that is already running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchKind {
    Launch,
    Attach,
}

impl LaunchKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::Attach => "attach",
        }
    }
}

impl std::fmt::Display for LaunchKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a listed configuration cannot be started.
///
/// Separate from listing on purpose: everything here is a perfectly valid
/// configuration for the tool that wrote it, and only lazydap's own limits
/// make it unrunnable. The distinction is what lets the error say "lazydap
/// cannot do this yet" rather than "your file is wrong".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotRunnable {
    /// An adapter lazydap does not ship (D013).
    UnsupportedAdapter { adapter_type: String },
    /// `request: "attach"`, which has no subcommand yet.
    AttachNotSupported,
    /// A `launch` configuration with nothing to launch.
    NoProgram,
    /// Variables nothing could expand, so the path is not a path.
    UnresolvedVariables { variables: Vec<String> },
    /// The configuration's arguments could not be read as a list.
    BadArguments { problem: String },
}

impl std::fmt::Display for NotRunnable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedAdapter { adapter_type } => write!(
                f,
                "it needs a `{adapter_type}` adapter, and lazydap ships codelldb only",
            ),
            Self::AttachNotSupported => {
                f.write_str("it attaches to a running process, which lazydap cannot do yet")
            }
            Self::NoProgram => f.write_str("it names no program to launch"),
            Self::UnresolvedVariables { variables } => write!(
                f,
                "nothing could expand {}, so its paths are not paths",
                variables.join(", "),
            ),
            Self::BadArguments { problem } => {
                write!(f, "its arguments could not be read: {problem}")
            }
        }
    }
}

impl std::error::Error for NotRunnable {}

impl LaunchConfig {
    /// The program this configuration would start, or why it cannot start one.
    ///
    /// The check and its result are one call so that a caller which passes it
    /// has the program in hand. Splitting them would leave every caller with a
    /// `program` that is still an `Option` it has just proved is `Some`, and
    /// the branch it then writes for the impossible case is a branch nobody
    /// can test.
    pub fn runnable_program(&self) -> Result<&std::path::Path, NotRunnable> {
        // Whatever the reader found comes first: it saw the file, and this
        // function is looking at what survived the reading.
        if let Some(blocked) = &self.blocked {
            return Err(blocked.clone());
        }
        if self.adapter.is_none() {
            return Err(NotRunnable::UnsupportedAdapter {
                adapter_type: self.adapter_type.clone(),
            });
        }
        if self.kind == LaunchKind::Attach {
            return Err(NotRunnable::AttachNotSupported);
        }
        if !self.unresolved.is_empty() {
            return Err(NotRunnable::UnresolvedVariables {
                variables: self.unresolved.clone(),
            });
        }
        self.program.as_deref().ok_or(NotRunnable::NoProgram)
    }

    /// Why this configuration cannot be started, or `None` if it can.
    ///
    /// The same answer [`runnable_program`](Self::runnable_program) gives, so
    /// `launches list` and `launches run` cannot disagree about which
    /// configurations are runnable: the list marks what `run` would refuse
    /// using the function `run` itself calls.
    pub fn not_runnable(&self) -> Option<NotRunnable> {
        self.runnable_program().err()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LaunchConfig {
        LaunchConfig {
            name: "Debug binary".to_string(),
            adapter: Some(AdapterKind::Codelldb),
            adapter_type: "lldb".to_string(),
            kind: LaunchKind::Launch,
            program: Some(PathBuf::from("/p/build/hello")),
            args: Vec::new(),
            cwd: Some(PathBuf::from("/p")),
            env: BTreeMap::new(),
            stop_on_entry: false,
            source: LaunchConfigSource::VsCodeLaunchJson,
            unresolved: Vec::new(),
            blocked: None,
        }
    }

    #[test]
    fn a_complete_codelldb_configuration_is_runnable() {
        assert_eq!(config().not_runnable(), None);
        assert_eq!(
            config().runnable_program(),
            Ok(std::path::Path::new("/p/build/hello")),
            "the check hands back what it validated, so no caller re-unwraps it",
        );
    }

    #[test]
    fn a_configuration_for_an_adapter_we_do_not_ship_says_which_one() {
        let config = LaunchConfig {
            adapter: None,
            adapter_type: "python".to_string(),
            ..config()
        };
        let reason = config.not_runnable().expect("debugpy is not built yet");
        assert!(reason.to_string().contains("python"), "got: {reason}");
    }

    #[test]
    fn an_attach_configuration_is_listed_but_not_runnable() {
        let config = LaunchConfig {
            kind: LaunchKind::Attach,
            ..config()
        };
        assert_eq!(config.not_runnable(), Some(NotRunnable::AttachNotSupported));
    }

    #[test]
    fn an_unexpanded_variable_stops_the_launch_rather_than_becoming_a_path() {
        // `${command:pickProcess}` as a literal directory name would launch
        // nothing, or worse, something else.
        let config = LaunchConfig {
            unresolved: vec!["${command:pickProcess}".to_string()],
            ..config()
        };
        let reason = config.not_runnable().expect("nothing expanded it");
        assert!(reason.to_string().contains("pickProcess"), "got: {reason}");
    }

    #[test]
    fn a_problem_the_reader_found_is_reported_ahead_of_anything_else() {
        // The fields left behind look fine — an unterminated quote leaves no
        // arguments to look wrong — so only what the reader saw can say this.
        let config = LaunchConfig {
            blocked: Some(NotRunnable::BadArguments {
                problem: "unterminated \" quote".to_string(),
            }),
            ..config()
        };
        let reason = config.not_runnable().expect("the reader refused it");
        assert!(reason.to_string().contains("unterminated"), "got: {reason}");
    }

    #[test]
    fn a_launch_configuration_with_no_program_is_refused_before_the_adapter_starts() {
        let config = LaunchConfig {
            program: None,
            ..config()
        };
        assert_eq!(config.not_runnable(), Some(NotRunnable::NoProgram));
    }
}
