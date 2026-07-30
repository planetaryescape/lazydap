//! DAP shapes in, lazydap shapes out.
//!
//! This is where camelCase, `variablesReference`, and DAP's habit of using
//! `0` to mean "absent" stop. Everything past this module works in
//! [`lazydap_core`] types (`ARCHITECTURE.md`, anti-pattern 4).

use lazydap_core::{
    AdapterBreakpoint, Breakpoint, EvalResult, Scope, SourceRef, StackFrame, StepKind, ThreadInfo,
    Variable,
};
use lazydap_dap::{
    DapScope, DapSource, DapStackFrame, DapThread, DapVariable, EvaluateResponse, SourceBreakpoint,
};
use std::path::PathBuf;

/// The DAP command that performs a step.
pub fn step_command(kind: StepKind) -> &'static str {
    match kind {
        // DAP calls step-over `next`, which is why `lazydap step` accepts
        // `next` as an alias: half the world learned the gdb name.
        StepKind::Over => "next",
        StepKind::In => "stepIn",
        StepKind::Out => "stepOut",
    }
}

pub fn stack_frame(frame: DapStackFrame) -> StackFrame {
    StackFrame {
        id: frame.id,
        name: frame.name,
        source: frame.source.map(source_ref),
        line: frame.line,
        column: frame.column,
    }
}

fn source_ref(source: DapSource) -> SourceRef {
    SourceRef {
        name: source.name,
        path: source.path.map(PathBuf::from),
        // DAP spells "not a reference" as `0`. Passing that on would have
        // clients asking the adapter for source number zero.
        source_reference: source.source_reference.filter(|reference| *reference != 0),
    }
}

pub fn scope(scope: DapScope) -> Scope {
    Scope {
        name: scope.name,
        variables_reference: scope.variables_reference,
        expensive: scope.expensive,
        named_variables: scope.named_variables,
        indexed_variables: scope.indexed_variables,
    }
}

pub fn variable(variable: DapVariable) -> Variable {
    Variable {
        name: variable.name,
        value: variable.value,
        type_name: variable.type_name,
        variables_reference: variable.variables_reference,
        named_variables: variable.named_variables,
        indexed_variables: variable.indexed_variables,
    }
}

pub fn eval_result(response: EvaluateResponse) -> EvalResult {
    EvalResult {
        value: response.result,
        type_name: response.type_name,
        variables_reference: response.variables_reference,
    }
}

pub fn thread_info(thread: DapThread) -> ThreadInfo {
    ThreadInfo {
        id: thread.id,
        name: if thread.name.is_empty() {
            format!("thread {}", thread.id)
        } else {
            thread.name
        },
    }
}

/// What to send for one of our breakpoints.
pub fn source_breakpoint(breakpoint: &Breakpoint) -> SourceBreakpoint {
    SourceBreakpoint {
        line: breakpoint.line,
        column: breakpoint.column,
        condition: breakpoint.condition.clone(),
        hit_condition: breakpoint.hit_condition.clone(),
        log_message: breakpoint.log_message.clone(),
    }
}

/// The location an adapter says it *could* have bound, out of the message it
/// sent when it declined to bind the one we asked for (quirk 8).
///
/// codelldb writes exactly this when the path we sent and the path in the
/// debug info are two spellings of one file:
///
/// ```text
/// Breakpoint at /private/tmp/demo/hello.c:6 could not be resolved, but a
/// valid location was found at /tmp/demo/hello.c:6
/// ```
///
/// Reading a human-readable message is as brittle as it looks, so the parse is
/// deliberately narrow: the text after the *last* `found at`, split at the
/// last colon, and only if what follows that colon is a line number and
/// nothing else. Anything the shape does not fit yields `None`, which costs
/// only the retry — the breakpoint is left exactly as unverified as it already
/// was. The caller then has to prove the suggestion names the same file before
/// acting on it.
pub fn suggested_location(message: &str) -> Option<PathBuf> {
    let (_, rest) = message.rsplit_once("found at ")?;
    let (path, line) = rest.rsplit_once(':')?;
    let line = line.trim();
    if line.is_empty() || !line.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Pair a `setBreakpoints` response with what we asked for.
///
/// DAP requires the response array to match the request array element for
/// element, which is the only thing tying an adapter id back to one of ours —
/// the response itself carries no reference to the request. A short or long
/// array means the adapter broke that contract, so anything unmatched is
/// reported without an id rather than guessed at.
pub fn reconcile_breakpoints(
    requested: &[Breakpoint],
    returned: Vec<lazydap_dap::Breakpoint>,
) -> Vec<AdapterBreakpoint> {
    returned
        .into_iter()
        .enumerate()
        .map(|(index, adapter)| AdapterBreakpoint {
            id: requested.get(index).map(|breakpoint| breakpoint.id),
            adapter_id: adapter.id,
            verified: adapter.verified,
            line: adapter.line,
            message: adapter.message,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazydap_core::BreakpointId;

    fn breakpoint(id: u32, line: u32) -> Breakpoint {
        Breakpoint {
            id: BreakpointId(id),
            source: PathBuf::from("/tmp/main.c"),
            line,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: true,
        }
    }

    fn returned(id: i64, verified: bool, line: Option<u32>) -> lazydap_dap::Breakpoint {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "verified": verified,
            "line": line,
        }))
        .expect("deserialise")
    }

    #[test]
    fn a_set_breakpoints_response_is_paired_with_the_request_by_position() {
        let requested = [breakpoint(7, 10), breakpoint(8, 20)];
        let reconciled = reconcile_breakpoints(
            &requested,
            vec![returned(1, true, Some(10)), returned(2, false, None)],
        );

        assert_eq!(reconciled[0].id, Some(BreakpointId(7)));
        assert_eq!(reconciled[0].adapter_id, Some(1));
        assert!(reconciled[0].verified);
        assert_eq!(reconciled[1].id, Some(BreakpointId(8)));
        assert!(!reconciled[1].verified);
    }

    #[test]
    fn an_adapter_that_returns_more_than_it_was_asked_for_gets_no_id_guessed() {
        let reconciled = reconcile_breakpoints(
            &[breakpoint(7, 10)],
            vec![returned(1, true, None), returned(2, true, None)],
        );

        assert_eq!(
            reconciled[1].id, None,
            "guessing would attach a stranger's id"
        );
        assert_eq!(reconciled[1].adapter_id, Some(2), "and it is still legible");
    }

    #[test]
    fn the_location_codelldb_suggests_is_read_out_of_its_message() {
        // Captured verbatim from a real run under /tmp on macOS (quirk 8).
        let suggested = suggested_location(
            "Breakpoint at /private/tmp/lazydap-demo/hello.c:6 could not be resolved, \
             but a valid location was found at /tmp/lazydap-demo/hello.c:6",
        );
        assert_eq!(
            suggested,
            Some(PathBuf::from("/tmp/lazydap-demo/hello.c")),
            "the two paths differ only by /private, which is easy to read past",
        );
    }

    #[test]
    fn a_message_with_no_suggestion_in_it_yields_nothing() {
        assert_eq!(
            suggested_location("Breakpoint at main.c:6 could not be resolved"),
            None,
        );
    }

    #[test]
    fn a_suggestion_without_a_line_number_is_not_a_location() {
        // The parse is narrow on purpose: reading a human-readable message is
        // brittle, and a wrong answer here re-sends breakpoints somewhere.
        assert_eq!(
            suggested_location("a valid location was found at /tmp/x"),
            None
        );
        assert_eq!(
            suggested_location("a valid location was found at /tmp/x:somewhere"),
            None,
        );
    }

    #[test]
    fn a_source_reference_of_zero_means_there_is_no_reference() {
        let frame = stack_frame(
            serde_json::from_value(serde_json::json!({
                "id": 1,
                "name": "main",
                "source": {"name": "main.c", "path": "/tmp/main.c", "sourceReference": 0},
                "line": 19,
                "column": 5,
            }))
            .expect("deserialise"),
        );

        let source = frame.source.expect("a source");
        assert_eq!(source.source_reference, None, "0 is DAP for absent");
        assert_eq!(source.path, Some(PathBuf::from("/tmp/main.c")));
    }

    #[test]
    fn a_thread_the_adapter_did_not_name_gets_a_usable_one() {
        let thread = thread_info(
            serde_json::from_value(serde_json::json!({"id": 26187878})).expect("deserialise"),
        );
        assert_eq!(thread.name, "thread 26187878");
    }

    #[test]
    fn step_over_is_dap_s_next_because_half_the_world_learned_the_gdb_name() {
        assert_eq!(step_command(StepKind::Over), "next");
        assert_eq!(step_command(StepKind::In), "stepIn");
        assert_eq!(step_command(StepKind::Out), "stepOut");
    }
}
