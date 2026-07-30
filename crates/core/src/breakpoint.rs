use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::ParseIntError;
use std::path::PathBuf;
use std::str::FromStr;

/// Identifies one lazydap breakpoint, for as long as the project keeps it.
///
/// A small integer rather than a UUID, and deliberately not the adapter's own
/// id. It is typed by humans (`lazydap break --remove --id 3`), piped through
/// `xargs`, and read back out of `.lazydap/state.toml`, so it has to be short
/// and stable across daemon restarts. Adapter ids are neither: codelldb hands
/// out fresh ones every session. See D031.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BreakpointId(pub u32);

impl fmt::Display for BreakpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for BreakpointId {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.trim().parse().map(Self)
    }
}

/// A breakpoint as lazydap knows it: project state that outlives any session.
///
/// This is the persisted shape (`.lazydap/state.toml`). What the *adapter*
/// currently thinks of it — verified or not, moved to another line — is
/// separate, because it is only true while a session is live. See
/// [`AdapterBreakpoint`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breakpoint {
    pub id: BreakpointId,
    /// Absolute path. Resolved by the client, which is the process that knows
    /// what the user's relative path was relative *to*.
    pub source: PathBuf,
    /// 1-based, matching every editor and the `linesStartAt1` we negotiate.
    pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// A log point: prints instead of pausing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

impl Breakpoint {
    /// `path:line`, the way the user wrote it and the way tables show it.
    pub fn location(&self) -> String {
        format!("{}:{}", self.source.display(), self.line)
    }
}

/// What the adapter says about a breakpoint right now.
///
/// codelldb verifies lazily: the `setBreakpoints` response can say
/// `verified: false` and a later event flips it, sometimes moving the line to
/// the nearest one with code. Both matter to a caller who just asked for a
/// breakpoint and wants to know whether it took.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterBreakpoint {
    /// Ours, when the adapter's id maps to one we know about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<BreakpointId>,
    /// The adapter's own id, kept so an unmapped update is still legible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<i64>,
    pub verified: bool,
    /// Where the adapter actually put it, when that differs from what we asked
    /// for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A breakpoint plus the adapter's current opinion of it. What `lazydap break`
/// and `lazydap break --list` print.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreakpointStatus {
    #[serde(flatten)]
    pub breakpoint: Breakpoint,
    /// `false` when no session has applied it yet, which is also the honest
    /// answer: nothing has checked that the line exists.
    pub verified: bool,
    /// Set when the adapter moved it to a different line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl BreakpointStatus {
    /// An unapplied breakpoint: the state every one is in before a session
    /// exists.
    pub fn unverified(breakpoint: Breakpoint) -> Self {
        Self {
            breakpoint,
            verified: false,
            adapter_line: None,
            message: None,
        }
    }

    /// Fold in what the adapter reported.
    pub fn apply(&mut self, update: &AdapterBreakpoint) {
        self.verified = update.verified;
        self.adapter_line = update.line.filter(|line| *line != self.breakpoint.line);
        if update.message.is_some() {
            self.message = update.message.clone();
        }
    }

    /// The line the debugger will actually stop on.
    pub fn effective_line(&self) -> u32 {
        self.adapter_line.unwrap_or(self.breakpoint.line)
    }
}

/// A `file:line` as typed on the command line.
///
/// Parsed client-side so a typo fails before a daemon is started, and so the
/// path is resolved against the user's working directory rather than the
/// daemon's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub source: PathBuf,
    pub line: u32,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.source.display(), self.line)
    }
}

impl FromStr for Location {
    type Err = BadLocation;

    /// Splits on the *last* colon, so a Windows-style or otherwise
    /// colon-bearing path does not lose its head to the line number.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (source, line) = s.rsplit_once(':').ok_or_else(|| BadLocation {
            input: s.to_string(),
            detail: "expected `file:line`".to_string(),
        })?;
        if source.is_empty() {
            return Err(BadLocation {
                input: s.to_string(),
                detail: "the file part is empty".to_string(),
            });
        }
        let line: u32 = line.trim().parse().map_err(|_| BadLocation {
            input: s.to_string(),
            detail: format!("`{line}` is not a line number"),
        })?;
        if line == 0 {
            return Err(BadLocation {
                input: s.to_string(),
                detail: "lines are numbered from 1".to_string(),
            });
        }
        Ok(Self {
            source: PathBuf::from(source),
            line,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadLocation {
    pub input: String,
    pub detail: String,
}

impl fmt::Display for BadLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not a location: {}", self.input, self.detail)
    }
}

impl std::error::Error for BadLocation {}

#[cfg(test)]
mod tests {
    use super::*;

    fn breakpoint(line: u32) -> Breakpoint {
        Breakpoint {
            id: BreakpointId(1),
            source: PathBuf::from("/tmp/main.c"),
            line,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: true,
        }
    }

    #[test]
    fn a_breakpoint_id_round_trips_as_a_bare_number() {
        let json = serde_json::to_string(&BreakpointId(7)).expect("serialise");
        assert_eq!(json, "7", "got: {json}");
        assert_eq!("7".parse::<BreakpointId>().expect("parse"), BreakpointId(7));
    }

    #[test]
    fn a_location_splits_on_the_last_colon_so_paths_survive() {
        let location: Location = "src/a:b/main.c:42".parse().expect("parse");
        assert_eq!(location.source, PathBuf::from("src/a:b/main.c"));
        assert_eq!(location.line, 42);
    }

    #[test]
    fn a_location_without_a_line_is_rejected_with_the_input_quoted() {
        let error = "main.c".parse::<Location>().expect_err("no line number");
        assert_eq!(error.input, "main.c");
        assert!(error.to_string().contains("file:line"), "got: {error}");
    }

    #[test]
    fn line_zero_is_rejected_because_lines_are_numbered_from_one() {
        let error = "main.c:0".parse::<Location>().expect_err("no line zero");
        assert!(error.to_string().contains("from 1"), "got: {error}");
    }

    #[test]
    fn a_moved_breakpoint_reports_where_it_actually_landed() {
        let mut status = BreakpointStatus::unverified(breakpoint(40));
        assert_eq!(status.effective_line(), 40);

        status.apply(&AdapterBreakpoint {
            id: Some(BreakpointId(1)),
            adapter_id: Some(9),
            verified: true,
            line: Some(42),
            message: None,
        });

        assert!(status.verified);
        assert_eq!(status.adapter_line, Some(42));
        assert_eq!(status.effective_line(), 42);
        assert_eq!(
            status.breakpoint.line, 40,
            "the line the user asked for is still what we persist",
        );
    }

    #[test]
    fn a_breakpoint_the_adapter_left_where_it_was_reports_no_move() {
        let mut status = BreakpointStatus::unverified(breakpoint(19));
        status.apply(&AdapterBreakpoint {
            id: Some(BreakpointId(1)),
            adapter_id: Some(1),
            verified: true,
            line: Some(19),
            message: None,
        });

        assert_eq!(
            status.adapter_line, None,
            "reporting a move to the same line would be noise",
        );
    }

    #[test]
    fn an_optional_field_is_omitted_rather_than_written_as_null() {
        let json = serde_json::to_string(&breakpoint(19)).expect("serialise");
        assert!(!json.contains("null"), "got: {json}");
        assert!(json.contains(r#""enabled":true"#), "got: {json}");
    }

    #[test]
    fn a_state_file_entry_without_enabled_reads_as_enabled() {
        // Hand-edited `.lazydap/state.toml` is a supported way to add a
        // breakpoint (D006), so the minimum sensible entry has to work.
        let breakpoint: Breakpoint =
            toml::from_str("id = 3\nsource = \"/tmp/main.c\"\nline = 19\n").expect("deserialise");
        assert!(breakpoint.enabled);
        assert_eq!(breakpoint.id, BreakpointId(3));
    }
}
