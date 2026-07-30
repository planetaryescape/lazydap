//! Prints the wire form of representative protocol messages.
//!
//! The documentation site shows JSON frames a client author will copy. Writing
//! those by hand gets the serde representation subtly wrong — a unit variant
//! serialises as `"Ping"`, not `{"Ping":null}`, and a client built from the
//! wrong shape is answered `BadRequest`. So the site pastes this program's
//! output instead of anybody's recollection of it.
//!
//! ```sh
//! cargo run -p lazydap-protocol --example wire_examples
//! ```
//!
//! Re-run it and update `site/src/content/docs/reference/protocol.md` whenever
//! the types change.

use std::collections::BTreeMap;
use std::path::PathBuf;

use lazydap_core::{AdapterKind, StepKind};
use lazydap_protocol::{
    Event, EventKind, IpcMessage, LAZYDAP_PROTOCOL_VERSION, LaunchRequest, Request, WaitMode,
};

fn show(label: &str, message: &IpcMessage) {
    let json = serde_json::to_string_pretty(message).expect("serialise");
    println!("--- {label} ---");
    println!("{json}");
    println!();
}

fn main() {
    println!("LAZYDAP_PROTOCOL_VERSION = {LAZYDAP_PROTOCOL_VERSION}\n");

    // Every field-less request looks like this. The mistake worth preventing is
    // assuming a unit variant needs a null payload.
    show("Ping", &IpcMessage::request(1, Request::Ping));
    show("Status", &IpcMessage::request(2, Request::Status));
    show("Shutdown", &IpcMessage::request(3, Request::Shutdown));
    show(
        "BreakpointList",
        &IpcMessage::request(4, Request::BreakpointList),
    );

    show(
        "Doctor",
        &IpcMessage::request(
            5,
            Request::Doctor {
                check_adapters: true,
                check_state: true,
            },
        ),
    );

    show(
        "Launch",
        &IpcMessage::request(
            6,
            Request::Launch(LaunchRequest {
                adapter: AdapterKind::Codelldb,
                program: PathBuf::from("/Users/you/lazydap-demo/hello"),
                args: vec![],
                cwd: PathBuf::from("/Users/you/lazydap-demo"),
                env: BTreeMap::new(),
                stop_on_entry: true,
            }),
        ),
    );

    // The wait mode is the interesting part: `None` means the daemon default.
    let session_id = Default::default();
    show(
        "Continue (waiting, daemon default timeout)",
        &IpcMessage::request(
            7,
            Request::Continue {
                session_id,
                thread_id: None,
                wait: WaitMode::Wait { timeout_ms: None },
                all_threads: false,
            },
        ),
    );

    show(
        "Step (over, not waiting)",
        &IpcMessage::request(
            8,
            Request::Step {
                session_id,
                thread_id: None,
                kind: StepKind::Over,
                wait: WaitMode::NoWait,
            },
        ),
    );

    // Channel names are snake_case on the wire even though the Rust variants
    // are not.
    show(
        "Subscribe",
        &IpcMessage::request(
            9,
            Request::Subscribe {
                channels: vec![
                    EventKind::Stopped,
                    EventKind::Output,
                    EventKind::SessionEnded,
                ],
            },
        ),
    );

    // Events carry id 0.
    show(
        "Event: Continued",
        &IpcMessage::event(Event::Continued {
            session_id,
            thread_id: Some(27982711),
            all_threads_continued: true,
        }),
    );

    println!("--- every Request variant name, in declaration order ---");
    for name in REQUEST_VARIANTS {
        println!("{name}");
    }
    println!();

    println!("--- every EventKind, as it appears on the wire ---");
    for kind in [
        EventKind::SessionStarted,
        EventKind::SessionEnded,
        EventKind::Stopped,
        EventKind::Continued,
        EventKind::Output,
        EventKind::BreakpointUpdated,
        EventKind::ThreadChanged,
    ] {
        println!("{}", serde_json::to_string(&kind).expect("serialise"));
    }
}

/// Kept beside the enum it mirrors. If you add a `Request` variant and forget
/// this list, the compiler will not tell you — but the site's inventory is
/// pasted from here, so a missing name shows up as a missing row.
const REQUEST_VARIANTS: &[&str] = &[
    "Ping",
    "Status",
    "Shutdown",
    "Version",
    "Doctor",
    "Launch",
    "Disconnect",
    "Continue",
    "Step",
    "Pause",
    "Threads",
    "StackTrace",
    "Scopes",
    "Variables",
    "Eval",
    "Output",
    "BreakpointList",
    "BreakpointAdd",
    "BreakpointRemove",
    "BreakpointToggle",
    "Subscribe",
];
