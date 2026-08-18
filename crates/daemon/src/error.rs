use lazydap_protocol::{ErrorCode, IpcError};

/// Exit codes, as documented in AGENTS.md. Agents branch on these, so they are
/// part of the product surface.
pub mod exit {
    pub const GENERAL: u8 = 1;
    pub const USAGE: u8 = 2;
    pub const DAEMON_UNREACHABLE: u8 = 3;
    pub const ADAPTER_NOT_FOUND: u8 = 4;
}

/// A command that did not work.
///
/// `label` is the machine-readable name that goes into the stderr JSON. For
/// anything the daemon reported it is the [`ErrorCode`] it sent; failures that
/// never reached a daemon get their own labels, because "DaemonInternalError"
/// would be a lie when there was no daemon.
#[derive(Debug, thiserror::Error)]
#[error("{source}")]
pub struct CliError {
    pub label: &'static str,
    pub exit_code: u8,
    #[source]
    pub source: anyhow::Error,
    /// Free-form context for the stderr JSON. Carried rather than recovered
    /// from `source`, because not every failure worth describing is an
    /// [`IpcError`] — a usage error is not one, and used to have to pretend.
    pub details: serde_json::Value,
}

/// `{}` — what a failure with nothing more to say carries.
fn no_details() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

impl CliError {
    /// The daemon could not be started or contacted.
    pub fn unreachable(source: impl Into<anyhow::Error>) -> Self {
        Self {
            label: "DaemonUnreachable",
            exit_code: exit::DAEMON_UNREACHABLE,
            source: source.into(),
            details: no_details(),
        }
    }

    /// A failure in this process that nobody has classified further.
    pub fn general(source: impl Into<anyhow::Error>) -> Self {
        Self {
            label: "DaemonInternalError",
            exit_code: exit::GENERAL,
            source: source.into(),
            details: no_details(),
        }
    }

    /// The command line did not make sense. Exit 2, the same as anything clap
    /// rejects, so a script cannot tell "you cannot combine those flags" from
    /// "there is no such flag" — and does not need to.
    pub fn usage(message: impl Into<String>) -> Self {
        Self::usage_with_details(message, no_details())
    }

    /// A usage error that can say which argument it means.
    ///
    /// The message is carried as plain text rather than as an [`IpcError`]:
    /// that type's `Display` prefixes the code, which printed
    /// `"message": "BadRequest: ..."` under `"error": "UsageError"` — two
    /// names for one mistake, and neither one the label a script branches on.
    pub fn usage_with_details(message: impl Into<String>, details: serde_json::Value) -> Self {
        Self {
            label: "UsageError",
            exit_code: exit::USAGE,
            source: anyhow::Error::msg(message.into()),
            details,
        }
    }

    /// The protocol version the other end reported, when this is a version
    /// mismatch.
    ///
    /// Needed to talk to a daemon that will not talk to us: a request stamped
    /// with its version is one it will accept.
    pub fn peer_protocol_version(&self) -> Option<u32> {
        self.source
            .downcast_ref::<IpcError>()
            .filter(|error| error.code == ErrorCode::VersionMismatch)
            .and_then(|error| error.details["daemon_version"].as_u64())
            .and_then(|version| u32::try_from(version).ok())
    }

    /// The stderr JSON body, in the shape AGENTS.md documents.
    pub fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "error": self.label,
            "message": format!("{:#}", self.source),
            "details": self.details,
        })
    }
}

impl From<IpcError> for CliError {
    fn from(error: IpcError) -> Self {
        let (label, exit_code) = classify(error.code);
        let details = error.details.clone();
        Self {
            label,
            exit_code,
            source: error.into(),
            details,
        }
    }
}

impl From<anyhow::Error> for CliError {
    fn from(source: anyhow::Error) -> Self {
        match source.downcast::<IpcError>() {
            Ok(error) => error.into(),
            Err(source) => Self::general(source),
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(source: std::io::Error) -> Self {
        Self::general(source)
    }
}

/// Failing to serialise our own output is a bug in this process, not something
/// the daemon or the adapter did.
impl From<serde_json::Error> for CliError {
    fn from(source: serde_json::Error) -> Self {
        Self::general(source)
    }
}

impl From<lazydap_config::ConfigError> for CliError {
    fn from(source: lazydap_config::ConfigError) -> Self {
        Self::from(lazydap_protocol::IpcError::new(
            lazydap_protocol::ErrorCode::InvalidLaunchConfig,
            source.to_string(),
        ))
    }
}

/// A `.vscode/launch.json` that cannot be read is the user's file being wrong,
/// not this process failing. `general` would label it `DaemonInternalError`,
/// which is a lie about whose problem it is — and no daemon was involved.
impl From<lazydap_config::LaunchJsonError> for CliError {
    fn from(source: lazydap_config::LaunchJsonError) -> Self {
        Self::from(lazydap_protocol::IpcError::new(
            lazydap_protocol::ErrorCode::InvalidLaunchConfig,
            source.to_string(),
        ))
    }
}

/// Every one of these is about a directory *lazydap* keeps its own files in —
/// the runtime directory holding the socket and the lock, or the data
/// directory holding the pid and the log. None of them is about the project
/// root, which is detected by walking up and never fails.
///
/// So they all mean the same thing to a caller: there is no daemon to talk to
/// and none can be started. Labelling them `InvalidProjectRoot` under exit 1
/// sent people to inspect a directory that was fine, and hid a retryable
/// failure inside the code for "the debugger said no".
impl From<lazydap_config::PathsError> for CliError {
    fn from(source: lazydap_config::PathsError) -> Self {
        Self::unreachable(source)
    }
}

/// What a protocol failure is called, and what the shell should see.
///
/// Exit codes live here rather than on `ErrorCode` itself: a TUI or a web
/// client speaks the same protocol and has no exit code to return. The match
/// is exhaustive so that adding a variant to `ErrorCode` forces a decision
/// about both, rather than defaulting quietly to 1.
fn classify(code: ErrorCode) -> (&'static str, u8) {
    match code {
        ErrorCode::AdapterNotFound => ("AdapterNotFound", exit::ADAPTER_NOT_FOUND),
        ErrorCode::VersionMismatch => ("VersionMismatch", exit::DAEMON_UNREACHABLE),
        ErrorCode::AdapterCrashed => ("AdapterCrashed", exit::GENERAL),
        ErrorCode::AdapterTimeout => ("AdapterTimeout", exit::GENERAL),
        ErrorCode::SessionNotFound => ("SessionNotFound", exit::GENERAL),
        ErrorCode::SessionAlreadyActive => ("SessionAlreadyActive", exit::GENERAL),
        ErrorCode::SessionNotPaused => ("SessionNotPaused", exit::GENERAL),
        // A caller mistake, like `BadRequest`, but not a *usage* one: the
        // command was spelled correctly and the handle was real when it was
        // issued. Exit 2 would tell a script to check its arguments, when what
        // it should do is ask again at this stop.
        ErrorCode::StaleHandle => ("StaleHandle", exit::GENERAL),
        ErrorCode::InvalidLaunchConfig => ("InvalidLaunchConfig", exit::GENERAL),
        ErrorCode::InvalidProjectRoot => ("InvalidProjectRoot", exit::GENERAL),
        ErrorCode::DapProtocolError => ("DapProtocolError", exit::GENERAL),
        ErrorCode::DaemonInternalError => ("DaemonInternalError", exit::GENERAL),
        ErrorCode::Unsupported => ("Unsupported", exit::GENERAL),
        ErrorCode::Timeout => ("Timeout", exit::GENERAL),
        ErrorCode::Cancelled => ("Cancelled", exit::GENERAL),
        ErrorCode::BadRequest => ("BadRequest", exit::GENERAL),
    }
}

pub type Result<T> = std::result::Result<T, CliError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_adapter_exits_four_and_names_itself_in_json() {
        let error: CliError = IpcError::new(ErrorCode::AdapterNotFound, "codelldb is not on PATH")
            .with_details(serde_json::json!({ "searched": ["/usr/bin"] }))
            .into();

        assert_eq!(error.exit_code, exit::ADAPTER_NOT_FOUND);

        let json = error.as_json();
        assert_eq!(json["error"], "AdapterNotFound");
        assert_eq!(json["message"], "AdapterNotFound: codelldb is not on PATH");
        assert_eq!(json["details"]["searched"][0], "/usr/bin");
    }

    #[test]
    fn an_unreachable_daemon_exits_three_rather_than_blaming_the_daemon() {
        let error = CliError::unreachable(anyhow::anyhow!("no socket at /tmp/x.sock"));
        assert_eq!(error.exit_code, exit::DAEMON_UNREACHABLE);
        assert_eq!(error.as_json()["error"], "DaemonUnreachable");
    }

    #[test]
    fn a_daemon_from_another_build_exits_three_so_a_script_can_tell_it_apart() {
        let error: CliError =
            IpcError::new(ErrorCode::VersionMismatch, "v1 client, v2 daemon").into();
        assert_eq!(error.exit_code, exit::DAEMON_UNREACHABLE);
        assert_eq!(error.as_json()["error"], "VersionMismatch");
    }

    #[test]
    fn a_version_mismatch_reports_the_other_end_s_version() {
        // What lets us address the outgoing daemon in a dialect it accepts.
        let error: CliError = IpcError::new(ErrorCode::VersionMismatch, "mismatch")
            .with_details(serde_json::json!({
                "client_version": 2,
                "daemon_version": 1,
            }))
            .into();

        assert_eq!(error.peer_protocol_version(), Some(1));
    }

    #[test]
    fn only_a_version_mismatch_carries_a_peer_version() {
        let error: CliError = IpcError::new(ErrorCode::SessionNotFound, "no session").into();
        assert_eq!(error.peer_protocol_version(), None);
    }

    #[test]
    fn a_usage_error_names_the_mistake_once() {
        // It used to be `"error": "UsageError"` over
        // `"message": "BadRequest: ..."` — two names for one mistake, and the
        // one in the message is not the one a script branches on.
        let error = CliError::usage("`--env FOO` is not a KEY=VALUE pair");

        let json = error.as_json();
        assert_eq!(json["error"], "UsageError");
        assert_eq!(json["message"], "`--env FOO` is not a KEY=VALUE pair");
        assert_eq!(error.exit_code, exit::USAGE);
    }

    #[test]
    fn a_usage_error_can_still_say_which_argument_it_means() {
        let error =
            CliError::usage_with_details("bad format", serde_json::json!({ "format": "ids" }));
        assert_eq!(error.as_json()["details"]["format"], "ids");
    }

    #[test]
    fn a_directory_lazydap_cannot_use_is_an_unreachable_daemon_not_a_bad_project() {
        // Every `PathsError` is about the runtime or data directory lazydap
        // keeps its socket, lock, pid and log in. Calling them
        // `InvalidProjectRoot` sent people to inspect a directory that was
        // fine, under exit 1, which tells a script not to retry.
        let error: CliError = lazydap_config::PathsError::SocketPathTooLong {
            len: 120,
            path: std::path::PathBuf::from("/very/long/socket.sock"),
        }
        .into();

        assert_eq!(error.exit_code, exit::DAEMON_UNREACHABLE);
        assert_eq!(error.as_json()["error"], "DaemonUnreachable");
    }

    #[test]
    fn an_unclassified_failure_is_a_general_error() {
        let error: CliError = anyhow::anyhow!("something came loose").into();
        assert_eq!(error.exit_code, exit::GENERAL);
        assert_eq!(error.as_json()["error"], "DaemonInternalError");
    }
}
