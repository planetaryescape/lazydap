//! `~/.config/lazydap/config.toml`: the user's own preferences.
//!
//! Per-user, never per-project — a project's state lives in
//! `.lazydap/state.toml` and belongs to `lazydap-store`. Absent by default:
//! lazydap does not write this file, and a machine without one runs on the
//! compiled-in defaults (`docs/blueprint/08-state-and-config.md`).
//!
//! # What this build actually reads
//!
//! Two things, because two things have a consumer:
//!
//! - `[adapter.<name>] command` — the first tier of adapter discovery (D026).
//!   Pointing lazydap at a specific codelldb build is the whole reason anyone
//!   reaches for this file.
//! - `[general] wait_timeout_seconds` — the default for `--wait`, under
//!   `--timeout` and `LAZYDAP_TIMEOUT`.
//!
//! The blueprint's schema is larger: themes, log rotation, socket directories,
//! output defaults. Those are not modelled here, deliberately. A field that
//! parses and then changes nothing is worse than one that does not exist,
//! because it reads like a setting that is being ignored — which it would be.
//! Unknown keys are therefore accepted and skipped rather than rejected, so a
//! config copied from the blueprint keeps working as fields land.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lazydap_core::AdapterKind;

/// Environment override for where the config file lives.
pub const CONFIG_PATH_ENV: &str = "LAZYDAP_CONFIG_PATH";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot locate a config directory for lazydap")]
    NoConfigDirectory,

    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not valid lazydap config: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

pub type Result<T> = std::result::Result<T, ConfigError>;

/// The user's preferences, or the defaults when there is no file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    general: General,
    /// Keyed by adapter name — `[adapter.codelldb]`. A `String` key rather
    /// than an [`AdapterKind`] so a config naming an adapter this build does
    /// not ship is skipped rather than failing the whole file: the same config
    /// has to keep working when debugpy lands (M18).
    #[serde(default)]
    adapter: BTreeMap<String, Adapter>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct General {
    wait_timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Adapter {
    command: Option<PathBuf>,
}

impl Config {
    /// Where the user pinned this adapter's binary, if they did.
    ///
    /// Tier one of D026. The path is returned as written, without checking it
    /// exists: discovery reports what it searched, and a pinned path that is
    /// wrong should appear in that list rather than being quietly skipped as
    /// though it had never been configured.
    pub fn adapter_command(&self, adapter: AdapterKind) -> Option<&Path> {
        self.adapter
            .get(adapter.as_str())
            .and_then(|adapter| adapter.command.as_deref())
    }

    /// The user's default `--wait` timeout in seconds, if they set one.
    pub fn wait_timeout_seconds(&self) -> Option<u64> {
        self.general.wait_timeout_seconds
    }
}

/// Where the config file lives: the environment override, else the platform's
/// config directory.
pub fn config_path() -> Result<PathBuf> {
    match env_path() {
        Some(path) => Ok(path),
        None => default_config_path(),
    }
}

fn default_config_path() -> Result<PathBuf> {
    dirs::config_dir()
        .map(|dir| dir.join("lazydap").join("config.toml"))
        .ok_or(ConfigError::NoConfigDirectory)
}

/// Read the user's config, or the defaults if there is none.
///
/// A missing file at the *default* location is normal — most people never
/// create one. A missing file at a path the user named with
/// `LAZYDAP_CONFIG_PATH` is not: they said where it was, so a typo there
/// should say so rather than silently running on defaults and leaving them to
/// work out why their pinned adapter is being ignored.
pub fn load_config() -> Result<Config> {
    if let Some(path) = env_path() {
        return load_config_from(&path);
    }

    let path = default_config_path()?;
    match std::fs::read_to_string(&path) {
        Ok(body) => parse(&path, &body),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(source) => Err(ConfigError::Read { path, source }),
    }
}

/// Read a config from a path, for tests and for anything that already knows
/// where the file is.
pub fn load_config_from(path: &Path) -> Result<Config> {
    let body = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse(path, &body)
}

fn parse(path: &Path, body: &str) -> Result<Config> {
    toml::from_str(body).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn env_path() -> Option<PathBuf> {
    std::env::var_os(CONFIG_PATH_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(body: &str) -> Config {
        parse(Path::new("/test/config.toml"), body).expect("parse")
    }

    #[test]
    fn an_empty_config_is_the_same_as_no_config() {
        let parsed = config("");
        assert_eq!(parsed.adapter_command(AdapterKind::Codelldb), None);
        assert_eq!(parsed.wait_timeout_seconds(), None);
    }

    #[test]
    fn a_pinned_adapter_binary_is_read_back_as_written() {
        let parsed = config(
            r#"
            [adapter.codelldb]
            command = "/opt/codelldb/codelldb"
            "#,
        );
        assert_eq!(
            parsed.adapter_command(AdapterKind::Codelldb),
            Some(Path::new("/opt/codelldb/codelldb")),
        );
    }

    #[test]
    fn a_default_wait_timeout_is_read_from_the_general_section() {
        let parsed = config(
            r#"
            [general]
            wait_timeout_seconds = 120
            "#,
        );
        assert_eq!(parsed.wait_timeout_seconds(), Some(120));
    }

    #[test]
    fn a_section_for_an_adapter_this_build_does_not_ship_is_kept_rather_than_refused() {
        // The same file has to work when debugpy lands (M18); failing on it
        // now would make upgrading lazydap break everyone who was ready early.
        let parsed = config(
            r#"
            [adapter.debugpy]
            command = "debugpy-adapter"

            [adapter.codelldb]
            command = "/usr/local/bin/codelldb"
            "#,
        );
        assert_eq!(
            parsed.adapter_command(AdapterKind::Codelldb),
            Some(Path::new("/usr/local/bin/codelldb")),
        );
    }

    #[test]
    fn the_blueprint_s_wider_schema_parses_even_though_most_of_it_does_nothing_yet() {
        // Copied from docs/blueprint/08-state-and-config.md. Somebody will,
        // and every key this build ignores must stay ignorable.
        let parsed = config(
            r#"
            version = 1

            [general]
            default_adapter = "codelldb"
            log_level = "info"
            wait_timeout_seconds = 45

            [tui]
            keymap = "vim"
            theme = "default"

            [output]
            default_format = "auto"

            [adapter.codelldb]
            command = "/usr/local/bin/codelldb"
            extra_args = []
            "#,
        );
        assert_eq!(parsed.wait_timeout_seconds(), Some(45));
        assert_eq!(
            parsed.adapter_command(AdapterKind::Codelldb),
            Some(Path::new("/usr/local/bin/codelldb")),
        );
    }

    #[test]
    fn a_malformed_config_names_the_file_and_the_problem() {
        let error = parse(Path::new("/test/config.toml"), "[general\nbroken")
            .expect_err("that is not TOML");

        assert!(matches!(error, ConfigError::Parse { .. }), "got: {error:?}",);
        assert!(
            error.to_string().contains("/test/config.toml"),
            "got: {error}"
        );
    }

    #[test]
    fn a_wrongly_typed_value_is_a_parse_error_rather_than_a_silent_default() {
        let error = parse(
            Path::new("/test/config.toml"),
            "[general]\nwait_timeout_seconds = \"soon\"",
        )
        .expect_err("that is not a number");
        assert!(matches!(error, ConfigError::Parse { .. }), "got: {error:?}");
    }

    #[test]
    fn a_named_config_that_is_not_there_is_an_error_rather_than_the_defaults() {
        // `load_config` treats a missing file at the *default* path as "no
        // config", because most machines have none. A path somebody named is
        // a different claim, and a typo in it must not read as defaults.
        let error = load_config_from(Path::new("/nowhere/at/all/config.toml"))
            .expect_err("load_config_from is the explicit spelling, and must not invent a file");
        assert!(matches!(error, ConfigError::Read { .. }), "got: {error:?}");
    }
}
