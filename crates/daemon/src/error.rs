use lazydap_protocol::{ErrorCode, IpcError};

/// Exit codes, as documented in AGENTS.md. Agents branch on these, so they are
/// part of the product surface.
pub mod exit {
    pub const GENERAL: u8 = 1;
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
}

impl CliError {
    /// The daemon could not be started or contacted.
    pub fn unreachable(source: impl Into<anyhow::Error>) -> Self {
        Self {
            label: "DaemonUnreachable",
            exit_code: exit::DAEMON_UNREACHABLE,
            source: source.into(),
        }
    }

    /// A failure in this process that nobody has classified further.
    pub fn general(source: impl Into<anyhow::Error>) -> Self {
        Self {
            label: "DaemonInternalError",
            exit_code: exit::GENERAL,
            source: source.into(),
        }
    }

    /// The stderr JSON body, in the shape AGENTS.md documents.
    pub fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "error": self.label,
            "message": format!("{:#}", self.source),
            "details": self.details(),
        })
    }

    fn details(&self) -> serde_json::Value {
        self.source
            .downcast_ref::<IpcError>()
            .map(|error| error.details.clone())
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()))
    }
}

impl From<IpcError> for CliError {
    fn from(error: IpcError) -> Self {
        let (label, exit_code) = classify(error.code);
        Self {
            label,
            exit_code,
            source: error.into(),
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

impl From<lazydap_config::PathsError> for CliError {
    fn from(source: lazydap_config::PathsError) -> Self {
        Self {
            label: "InvalidProjectRoot",
            exit_code: exit::GENERAL,
            source: source.into(),
        }
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
    fn an_unclassified_failure_is_a_general_error() {
        let error: CliError = anyhow::anyhow!("something came loose").into();
        assert_eq!(error.exit_code, exit::GENERAL);
        assert_eq!(error.as_json()["error"], "DaemonInternalError");
    }
}
