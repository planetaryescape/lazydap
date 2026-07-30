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
    /// The user's config, read once per invocation. The defaults when there
    /// is none, and also when the one there is cannot be read — see
    /// [`config_problem`](Self::config_problem).
    pub config: Config,
    /// Why the config could not be read, if it could not.
    ///
    /// **Not an error from `resolve`.** A typo in `config.toml` used to fail
    /// every command that parses one, which is every command — including
    /// `shutdown`, `disconnect`, `status` and `logs`, the four you reach for
    /// when a debuggee is running and something is wrong. Bricking the
    /// recovery tools over a misplaced bracket is the worst possible moment to
    /// be strict.
    ///
    /// So it is carried rather than raised. Commands that would *act* on the
    /// config refuse (`Command::needs_config`); the rest warn and carry on
    /// with the defaults, and `lazydap doctor` reports it as a failed check —
    /// which is the command whose job is telling you exactly this.
    pub config_problem: Option<String>,
}

impl Instance {
    /// Resolve from the working directory, honouring `--instance` over
    /// `LAZYDAP_INSTANCE` over the detected project root (D010).
    pub fn resolve(explicit: Option<&str>) -> Result<Self> {
        let cwd = std::env::current_dir().map_err(CliError::general)?;
        let name = paths::instance_name(&cwd, explicit);
        let (config, config_problem) = match lazydap_config::load_config() {
            Ok(config) => (config, None),
            Err(error) => (Config::default(), Some(error.to_string())),
        };

        Ok(Self {
            config,
            config_problem,
            project_root: paths::project_root(&cwd),
            socket: paths::socket_path(&name)?,
            lock: paths::lock_path(&name)?,
            pid: paths::pid_path(&name)?,
            log: paths::log_path(&name)?,
            name,
        })
    }
}
