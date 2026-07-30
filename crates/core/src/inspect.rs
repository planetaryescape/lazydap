//! What a paused program looks like: threads, frames, scopes, variables.
//!
//! These are lazydap's own shapes, not DAP's. They are deliberately thinner
//! than the DAP structures they are translated from — every field here is one
//! a client actually renders or branches on, and the rest stops at the adapter
//! seam (`ARCHITECTURE.md`, anti-pattern 4).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Where a frame's code lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Absent for frames the adapter can only serve by reference — inlined
    /// code, disassembly, a source it holds in memory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Non-zero when the source has to be fetched from the adapter rather than
    /// read off disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<i64>,
}

impl SourceRef {
    /// The most useful single string for this source: its path, else its name.
    pub fn label(&self) -> String {
        match (&self.path, &self.name) {
            (Some(path), _) => path.display().to_string(),
            (None, Some(name)) => name.clone(),
            (None, None) => "<unknown>".to_string(),
        }
    }
}

/// One frame of the call stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackFrame {
    /// The adapter's handle for this frame. Pass it to `scopes` and `eval`.
    /// Only valid until the program moves.
    pub id: i64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for StackFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.source {
            Some(source) => write!(f, "{} at {}:{}", self.name, source.label(), self.line),
            None => write!(f, "{} at line {}", self.name, self.line),
        }
    }
}

/// A named group of variables in a frame — locals, arguments, globals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub name: String,
    /// Pass to `lazydap variables --reference N` to expand it.
    pub variables_reference: i64,
    /// The adapter warns this one is slow to expand (globals, usually).
    pub expensive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_variables: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_variables: Option<u32>,
}

/// One variable, as a string the adapter formatted for display.
///
/// The value is always a string: a debugger's job is to render whatever the
/// target language has, and forcing that through a JSON number would lose
/// precision, pointers, and every aggregate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    /// Non-zero when this variable has children — a struct, an array, a
    /// pointer worth following. Pass it back to `lazydap variables`.
    pub variables_reference: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_variables: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_variables: Option<u32>,
}

/// The result of evaluating an expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalResult {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    pub variables_reference: i64,
}

/// One thread of the debuggee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadInfo {
    pub id: i64,
    pub name: String,
}

/// A thread starting or ending during a `--wait`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadUpdate {
    pub thread_id: i64,
    pub kind: ThreadUpdateKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadUpdateKind {
    Started,
    Exited,
}

/// Which way to step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// Run the next line, stepping over any call in it.
    Over,
    /// Step into the call on this line.
    In,
    /// Run until this function returns.
    Out,
}

impl StepKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Over => "over",
            Self::In => "in",
            Self::Out => "out",
        }
    }
}

impl fmt::Display for StepKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why an expression is being evaluated.
///
/// Not a formatting hint. codelldb reads `repl` as "this is a line typed at
/// the debugger console" and hands it to LLDB's *command* interpreter, where
/// `x` is the memory-read alias rather than your variable called `x`
/// (`docs/reference/codelldb-quirks.md`, quirk 7). `watch` and `hover` are the
/// contexts that evaluate an expression in the program's own language.
///
/// `Watch` is therefore the default: `lazydap eval "x + y"` is asking about
/// the program, not driving LLDB. `Repl` remains available for callers who do
/// want to run an adapter command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalContext {
    #[default]
    Watch,
    Repl,
    Hover,
}

impl EvalContext {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Repl => "repl",
            Self::Watch => "watch",
            Self::Hover => "hover",
        }
    }
}

impl fmt::Display for EvalContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for EvalContext {
    type Err = BadValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "repl" => Ok(Self::Repl),
            "watch" => Ok(Self::Watch),
            "hover" => Ok(Self::Hover),
            other => Err(BadValue {
                value: other.to_string(),
                expected: "repl, watch or hover",
            }),
        }
    }
}

/// Which half of a container to fetch. Big arrays are paged by index; structs
/// are named.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableFilter {
    /// Everything the adapter has.
    #[default]
    All,
    Named,
    Indexed,
}

impl VariableFilter {
    /// The DAP spelling, or `None` for "do not filter".
    pub fn as_dap(&self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Named => Some("named"),
            Self::Indexed => Some("indexed"),
        }
    }
}

impl std::str::FromStr for VariableFilter {
    type Err = BadValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "named" => Ok(Self::Named),
            "indexed" => Ok(Self::Indexed),
            other => Err(BadValue {
                value: other.to_string(),
                expected: "all, named or indexed",
            }),
        }
    }
}

/// A command-line value that is not one of the ones there are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadValue {
    pub value: String,
    /// What would have been accepted, phrased for a person reading an error.
    pub expected: &'static str,
}

impl fmt::Display for BadValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not one of {}", self.value, self.expected)
    }
}

impl std::error::Error for BadValue {}

/// How a `--wait` ended.
///
/// The three real stable states plus two synthetic ones: `Timeout` (nothing
/// settled in time, the program is still running) and `AdapterDied` (the
/// adapter process vanished, D022). Spelled exactly as AGENTS.md documents
/// them, because agents branch on this string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitOutcome {
    Paused,
    Exited,
    Terminated,
    Timeout,
    AdapterDied,
}

impl WaitOutcome {
    /// The same spelling the JSON uses. Kept beside the `serde` attribute so
    /// the table output and the pipeline cannot disagree about what happened.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::Exited => "exited",
            Self::Terminated => "terminated",
            Self::Timeout => "timeout",
            Self::AdapterDied => "adapter_died",
        }
    }

    /// Whether the session can still be used after this.
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Paused | Self::Timeout)
    }
}

impl fmt::Display for WaitOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wait_outcome_prints_the_way_it_serialises() {
        for outcome in [
            WaitOutcome::Paused,
            WaitOutcome::Exited,
            WaitOutcome::Terminated,
            WaitOutcome::Timeout,
            WaitOutcome::AdapterDied,
        ] {
            let json = serde_json::to_string(&outcome).expect("serialise");
            assert_eq!(json, format!("\"{}\"", outcome.as_str()), "got: {json}");
        }
    }

    #[test]
    fn only_a_pause_or_a_timeout_leaves_a_session_worth_talking_to() {
        assert!(WaitOutcome::Paused.is_live());
        assert!(
            WaitOutcome::Timeout.is_live(),
            "a timeout means the program is still running, not that it is gone",
        );
        assert!(!WaitOutcome::Exited.is_live());
        assert!(!WaitOutcome::AdapterDied.is_live());
    }

    #[test]
    fn a_frame_prints_where_it_is() {
        let frame = StackFrame {
            id: 42,
            name: "main".to_string(),
            source: Some(SourceRef {
                name: Some("main.c".to_string()),
                path: Some(PathBuf::from("/tmp/main.c")),
                source_reference: None,
            }),
            line: 19,
            column: 5,
        };
        assert_eq!(frame.to_string(), "main at /tmp/main.c:19");
    }

    #[test]
    fn a_frame_with_no_source_on_disk_still_prints() {
        let frame = StackFrame {
            id: 1,
            name: "__libc_start".to_string(),
            source: None,
            line: 0,
            column: 0,
        };
        assert_eq!(frame.to_string(), "__libc_start at line 0");
    }

    #[test]
    fn a_source_served_only_by_reference_falls_back_to_its_name() {
        let source = SourceRef {
            name: Some("@disassembly".to_string()),
            path: None,
            source_reference: Some(1001),
        };
        assert_eq!(source.label(), "@disassembly");
    }

    #[test]
    fn the_all_filter_asks_the_adapter_for_no_filter_at_all() {
        assert_eq!(VariableFilter::All.as_dap(), None);
        assert_eq!(VariableFilter::Indexed.as_dap(), Some("indexed"));
    }

    #[test]
    fn evaluating_defaults_to_asking_about_the_program_not_driving_the_debugger() {
        // codelldb sends a `repl` expression to LLDB's command interpreter,
        // where `x` means `memory read`. Verified live; see quirk 7.
        assert_eq!(EvalContext::default(), EvalContext::Watch);
    }

    #[test]
    fn an_evaluation_context_parses_from_the_word_a_person_would_type() {
        assert_eq!(
            "watch".parse::<EvalContext>().expect("parse"),
            EvalContext::Watch
        );
        assert_eq!(
            "REPL".parse::<EvalContext>().expect("parse"),
            EvalContext::Repl
        );
        assert!("nonsense".parse::<EvalContext>().is_err());
    }
}
