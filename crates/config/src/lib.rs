//! Where lazydap keeps its files, and which project a command belongs to.
//!
//! M5 needs only the path half of this crate: the socket, PID, lock and log
//! files, plus the project-root detection that keys one daemon per project
//! (D010). Config-file loading and `launch.json` import land with M15.

pub mod paths;

pub use paths::{PathsError, Result};
