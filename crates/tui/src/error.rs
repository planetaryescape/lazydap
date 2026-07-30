/// What can go wrong in the TUI.
///
/// The daemon crate wraps this into its own `CliError` for an exit code, so
/// nothing here decides one.
#[derive(Debug, thiserror::Error)]
pub enum TuiError {
    /// Raw mode, the alternate screen, a draw, or reading a key.
    ///
    /// The common one in practice is "not a terminal": `lazydap tui` with its
    /// input redirected cannot enable raw mode, and says so rather than
    /// drawing to a pipe.
    #[error("the terminal could not be driven: {0}")]
    Terminal(#[from] std::io::Error),

    #[error("cannot connect to the daemon socket at {socket}: {source}")]
    Connect {
        socket: String,
        source: std::io::Error,
    },

    /// The daemon went away before it said anything useful. Only reachable
    /// during the handshake: afterwards a lost connection is a `Msg`, because
    /// by then there is a screen to show it on.
    #[error("the daemon closed the connection during the handshake")]
    DaemonGone,

    #[error(
        "this lazydap speaks protocol v{ours}, the running daemon speaks v{daemon}; \
         run `lazydap shutdown` and try again"
    )]
    VersionMismatch { daemon: u32, ours: u32 },

    #[error("{0}")]
    Protocol(#[from] lazydap_protocol::IpcError),

    #[error("the daemon sent something unexpected: {0}")]
    UnexpectedFrame(String),
}

pub type Result<T> = std::result::Result<T, TuiError>;
