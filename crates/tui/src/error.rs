/// What can go wrong in the TUI.
///
/// Thin on purpose: the daemon crate wraps this into its own `CliError` for an
/// exit code, and everything the TUI itself can fail at is the terminal.
#[derive(Debug, thiserror::Error)]
pub enum TuiError {
    /// Raw mode, the alternate screen, a draw, or reading a key.
    ///
    /// The common one in practice is "not a terminal": `lazydap tui` with its
    /// input redirected cannot enable raw mode, and says so rather than
    /// drawing to a pipe.
    #[error("the terminal could not be driven: {0}")]
    Terminal(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, TuiError>;
