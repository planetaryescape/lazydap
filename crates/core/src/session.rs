use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Identifies one debug session for the lifetime of the daemon that owns it.
///
/// Every session-scoped IPC request carries one, even though v0.1 enforces a
/// single live session (D007): the constraint is the daemon's, not the
/// protocol's, so lifting it later leaves clients untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for SessionId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Which external debug adapter backs a session.
///
/// Three of them since M22: codelldb for compiled languages with debug info (C,
/// C++, Rust), debugpy for Python, delve for Go. The default is codelldb
/// because that is what a program lazydap cannot classify is most likely to be
/// — a native binary has no extension to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    #[default]
    Codelldb,
    Debugpy,
    Delve,
}

impl AdapterKind {
    /// Every adapter lazydap ships.
    ///
    /// A constant rather than a literal at each use site because the two
    /// callers that need all of them — `doctor`'s adapter sweep and the CLI's
    /// help text — are arrays the compiler cannot check for exhaustiveness. An
    /// adapter missing from `doctor` is invisible rather than broken, which is
    /// the kind of omission that survives a release.
    pub const ALL: &'static [Self] = &[Self::Codelldb, Self::Debugpy, Self::Delve];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Codelldb => "codelldb",
            Self::Debugpy => "debugpy",
            Self::Delve => "delve",
        }
    }

    /// Which adapter a program's filename says it needs, when it says
    /// anything.
    ///
    /// Only the extension is read. Sniffing a shebang or an ELF header would
    /// classify more programs, and would also mean opening a file the caller
    /// has not asked to debug yet — and getting it wrong silently, which a
    /// missing extension does not. `None` leaves the choice to the caller's
    /// `--adapter`, or to the default.
    ///
    /// `.go` is a *source* file and the others here are too, which is the same
    /// thing it has always meant: the extension says which toolchain owns the
    /// program, not that the file is directly executable. A compiled Go binary
    /// has no extension and so lands on codelldb, which can read its DWARF —
    /// `--adapter delve` is how a caller says otherwise.
    pub fn for_program(program: &std::path::Path) -> Option<Self> {
        match program.extension()?.to_str()? {
            "py" => Some(Self::Debugpy),
            "go" => Some(Self::Delve),
            "c" | "cc" | "cpp" | "cxx" | "rs" => Some(Self::Codelldb),
            _ => None,
        }
    }
}

impl fmt::Display for AdapterKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AdapterKind {
    type Err = UnknownAdapter;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "codelldb" | "lldb" => Ok(Self::Codelldb),
            // `python` is what a `launch.json` written for VS Code's older
            // Python extension says; `debugpy` is the current spelling. Both
            // name the same adapter.
            "debugpy" | "python" => Ok(Self::Debugpy),
            // `go` is what a `launch.json` written for the VS Code Go
            // extension says; `dlv` is what the binary is called. Both name
            // delve.
            "delve" | "dlv" | "go" => Ok(Self::Delve),
            other => Err(UnknownAdapter(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAdapter(pub String);

impl fmt::Display for UnknownAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shipped = AdapterKind::ALL
            .iter()
            .map(AdapterKind::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "unknown adapter: {} (lazydap ships {shipped})", self.0)
    }
}

impl std::error::Error for UnknownAdapter {}

/// Where a session is in its lifecycle. Serialises as a bare lower-case string
/// (`"paused"`, `"exited"`, ...) because that is the discriminator documented
/// for `--format json` consumers in AGENTS.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// Adapter spawned, launch handshake not finished.
    Initialising,
    Running,
    Paused,
    /// Debuggee exited on its own.
    Exited,
    /// Debuggee was killed, or the adapter reported `terminated`.
    Terminated,
    /// Adapter process vanished without saying goodbye (D022).
    AdapterDied,
}

impl SessionState {
    /// The same spelling the JSON uses.
    ///
    /// Kept next to the `serde` attribute above so the two cannot drift:
    /// deriving this from `Debug` would print `AdapterDied` where the JSON
    /// says `adapter_died`, and the table and the pipeline would disagree
    /// about what state a session is in.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Initialising => "initialising",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Exited => "exited",
            Self::Terminated => "terminated",
            Self::AdapterDied => "adapter_died",
        }
    }

    /// Stable states are the ones where querying stack/scopes is meaningful.
    pub fn is_stable(&self) -> bool {
        matches!(
            self,
            Self::Paused | Self::Exited | Self::Terminated | Self::AdapterDied
        )
    }

    /// Whether the session still owns a live adapter.
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Initialising | Self::Running | Self::Paused)
    }
}

/// Why the debuggee stopped.
///
/// Serialises as a bare string so that an adapter-specific reason we have not
/// modelled still reaches clients as `"reason": "whatever-it-said"` rather than
/// changing the shape of the field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", from = "String")]
pub enum PauseReason {
    Entry,
    Step,
    Breakpoint,
    Exception,
    Pause,
    Goto,
    FunctionBreakpoint,
    DataBreakpoint,
    InstructionBreakpoint,
    Other(String),
}

impl PauseReason {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Entry => "entry",
            Self::Step => "step",
            Self::Breakpoint => "breakpoint",
            Self::Exception => "exception",
            Self::Pause => "pause",
            Self::Goto => "goto",
            Self::FunctionBreakpoint => "function_breakpoint",
            Self::DataBreakpoint => "data_breakpoint",
            Self::InstructionBreakpoint => "instruction_breakpoint",
            Self::Other(reason) => reason,
        }
    }
}

impl fmt::Display for PauseReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for PauseReason {
    /// Accepts both DAP's camelCase spellings (`functionBreakpoint`) and our
    /// own snake_case ones, so a value survives a round trip through JSON.
    fn from(value: &str) -> Self {
        match squash(value).as_str() {
            "entry" => Self::Entry,
            "step" => Self::Step,
            "breakpoint" => Self::Breakpoint,
            "exception" => Self::Exception,
            "pause" => Self::Pause,
            "goto" => Self::Goto,
            "functionbreakpoint" => Self::FunctionBreakpoint,
            "databreakpoint" => Self::DataBreakpoint,
            "instructionbreakpoint" => Self::InstructionBreakpoint,
            _ => Self::Other(value.to_string()),
        }
    }
}

impl From<String> for PauseReason {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl From<PauseReason> for String {
    fn from(value: PauseReason) -> Self {
        value.as_str().to_string()
    }
}

/// Which stream a chunk of captured output came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", from = "String")]
pub enum OutputCategory {
    Stdout,
    Stderr,
    /// The adapter talking to the user, not the debuggee.
    Console,
    Telemetry,
    Other(String),
}

impl OutputCategory {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::Console => "console",
            Self::Telemetry => "telemetry",
            Self::Other(category) => category,
        }
    }

    /// Whether this chunk is the debuggee's own output rather than the
    /// adapter's chatter.
    pub fn is_debuggee(&self) -> bool {
        matches!(self, Self::Stdout | Self::Stderr)
    }
}

impl fmt::Display for OutputCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for OutputCategory {
    fn from(value: &str) -> Self {
        match squash(value).as_str() {
            "stdout" => Self::Stdout,
            "stderr" => Self::Stderr,
            "console" => Self::Console,
            "telemetry" => Self::Telemetry,
            _ => Self::Other(value.to_string()),
        }
    }
}

impl From<String> for OutputCategory {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl From<OutputCategory> for String {
    fn from(value: OutputCategory) -> Self {
        value.as_str().to_string()
    }
}

/// One piece of output the debuggee (or the adapter) produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputChunk {
    pub category: OutputCategory,
    pub output: String,
    /// Milliseconds since the Unix epoch. A plain integer rather than a
    /// formatted timestamp: no date dependency, and every language can read it.
    pub timestamp_ms: u64,
}

impl OutputChunk {
    pub fn new(category: OutputCategory, output: impl Into<String>) -> Self {
        Self {
            category,
            output: output.into(),
            timestamp_ms: now_ms(),
        }
    }
}

/// Why a session ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// A client asked for it.
    Disconnected,
    /// The debuggee ran to completion.
    Exited { exit_code: Option<i32> },
    /// The adapter reported `terminated`.
    Terminated,
    /// The adapter process disappeared; we synthesised the ending (D022).
    AdapterDied { detail: String },
}

/// Milliseconds since the Unix epoch, saturating at 0 for clocks set before it.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Lower-case and strip separators so `functionBreakpoint`,
/// `function_breakpoint` and `FUNCTION-BREAKPOINT` all parse the same.
fn squash(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_distinct_and_round_trip_as_strings() {
        let id = SessionId::new();
        assert_ne!(id, SessionId::new());

        let json = serde_json::to_string(&id).expect("serialise");
        assert_eq!(json, format!("\"{id}\""), "got: {json}");
        assert_eq!(id.to_string().parse::<SessionId>().expect("parse"), id);
    }

    #[test]
    fn session_state_serialises_as_a_bare_lower_case_string() {
        let json = serde_json::to_string(&SessionState::AdapterDied).expect("serialise");
        assert_eq!(json, r#""adapter_died""#, "got: {json}");
    }

    #[test]
    fn every_session_state_prints_the_way_it_serialises() {
        // The table output and the JSON output must name the same state the
        // same way, or a user comparing the two sees a contradiction.
        for state in [
            SessionState::Initialising,
            SessionState::Running,
            SessionState::Paused,
            SessionState::Exited,
            SessionState::Terminated,
            SessionState::AdapterDied,
        ] {
            let json = serde_json::to_string(&state).expect("serialise");
            assert_eq!(json, format!("\"{}\"", state.as_str()), "got: {json}");
        }
    }

    #[test]
    fn pause_reason_keeps_an_unmodelled_adapter_reason_as_a_string() {
        let reason = PauseReason::from("goroutine-yield");
        assert_eq!(reason, PauseReason::Other("goroutine-yield".into()));

        let json = serde_json::to_string(&reason).expect("serialise");
        assert_eq!(json, r#""goroutine-yield""#, "got: {json}");
    }

    #[test]
    fn pause_reason_reads_both_dap_and_lazydap_spellings() {
        assert_eq!(
            PauseReason::from("functionBreakpoint"),
            PauseReason::FunctionBreakpoint
        );
        assert_eq!(
            PauseReason::from("function_breakpoint"),
            PauseReason::FunctionBreakpoint
        );

        let round_tripped: PauseReason = serde_json::from_str(
            &serde_json::to_string(&PauseReason::FunctionBreakpoint).expect("serialise"),
        )
        .expect("deserialise");
        assert_eq!(round_tripped, PauseReason::FunctionBreakpoint);
    }

    #[test]
    fn output_category_distinguishes_debuggee_streams_from_adapter_chatter() {
        assert!(OutputCategory::from("stdout").is_debuggee());
        assert!(!OutputCategory::from("console").is_debuggee());
    }

    #[test]
    fn end_reason_carries_the_exit_code() {
        let json =
            serde_json::to_string(&EndReason::Exited { exit_code: Some(3) }).expect("serialise");
        assert_eq!(json, r#"{"exited":{"exit_code":3}}"#, "got: {json}");
    }

    #[test]
    fn only_live_states_own_an_adapter() {
        assert!(SessionState::Paused.is_live());
        assert!(SessionState::Paused.is_stable());
        assert!(!SessionState::Exited.is_live());
        assert!(!SessionState::Running.is_stable());
    }
}
