//! Where lazydap keeps things, and what the user asked for.
//!
//! Three separate jobs, deliberately in one crate because all three answer
//! "what does this machine and this project say?" without knowing anything
//! about sockets, adapters or sessions:
//!
//! - [`paths`] — the socket, PID, lock and log files, and the project-root
//!   detection that keys one daemon per project (D010, D024).
//! - [`file`] — `~/.config/lazydap/config.toml`, the user's own preferences.
//! - [`launch_json`] — `.vscode/launch.json`, imported read-only (D008).

pub mod file;
pub mod launch_json;
pub mod paths;

pub use file::{CONFIG_PATH_ENV, Config, ConfigError, config_path, load_config, load_config_from};
pub use launch_json::{Imported, LaunchJsonError};
pub use paths::PathsError;
