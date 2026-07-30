use crate::error::{CliError, Result};
use lazydap_config::{Config, paths};
use std::path::PathBuf;

/// The four files that belong to one project's daemon, resolved once so the
/// client and the daemon cannot disagree about where they are.
#[derive(Debug, Clone)]
pub struct Instance {
    pub name: String,
    /// Where `.lazydap/state.toml` lives (D024).
    ///
    /// Detected from the working directory — the daemon's, when it is the
    /// daemon asking. That works because a daemon is spawned by a client and
    /// inherits its directory, so both walk up from the same place and reach
    /// the same root.
    pub project_root: PathBuf,
    pub socket: PathBuf,
    pub lock: PathBuf,
    pub pid: PathBuf,
    pub log: PathBuf,
    /// `~/.config/lazydap/config.toml`, read once per invocation.
    ///
    /// Here rather than at each point of use so a malformed config is one
    /// error at the start of the command, not a different one depending on
    /// which subcommand happened to reach for a setting.
    pub config: Config,
}

impl Instance {
    /// Resolve from the working directory, honouring `--instance` over
    /// `LAZYDAP_INSTANCE` over the detected project root (D010).
    pub fn resolve(explicit: Option<&str>) -> Result<Self> {
        let cwd = std::env::current_dir().map_err(CliError::general)?;
        let name = paths::instance_name(&cwd, explicit);
        Ok(Self {
            config: lazydap_config::load_config()?,
            project_root: paths::project_root(&cwd),
            socket: paths::socket_path(&name)?,
            lock: paths::lock_path(&name)?,
            pid: paths::pid_path(&name)?,
            log: paths::log_path(&name)?,
            name,
        })
    }
}
