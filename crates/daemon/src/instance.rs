use crate::error::{CliError, Result};
use lazydap_config::paths;
use std::path::PathBuf;

/// The four files that belong to one project's daemon, resolved once so the
/// client and the daemon cannot disagree about where they are.
#[derive(Debug, Clone)]
pub struct Instance {
    pub name: String,
    pub socket: PathBuf,
    pub lock: PathBuf,
    pub pid: PathBuf,
    pub log: PathBuf,
}

impl Instance {
    /// Resolve from the working directory, honouring `--instance` over
    /// `LAZYDAP_INSTANCE` over the detected project root (D010).
    pub fn resolve(explicit: Option<&str>) -> Result<Self> {
        let cwd = std::env::current_dir().map_err(CliError::general)?;
        let name = paths::instance_name(&cwd, explicit);
        Ok(Self {
            socket: paths::socket_path(&name)?,
            lock: paths::lock_path(&name)?,
            pid: paths::pid_path(&name)?,
            log: paths::log_path(&name)?,
            name,
        })
    }
}
