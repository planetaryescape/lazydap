//! debugpy: Python.
//!
//! The second adapter, and the one that proved the seam. Where codelldb needs
//! a normalisation for nearly everything it does, debugpy follows the
//! specification closely enough that most of this module is the absence of
//! workarounds:
//!
//! - It speaks DAP over **stdio**, not TCP, and it is not an executable of its
//!   own: it is a module of a Python interpreter, run as
//!   `python3 -m debugpy.adapter`. That is why [`super::Spawn`] carries
//!   arguments and why discovery resolves an *interpreter*.
//! - It reports a stop-on-entry stop as `reason: "entry"`, which is what the
//!   specification says and what lazydap already calls it. codelldb's D033
//!   renaming simply never fires here — verified against debugpy 1.8.21, not
//!   assumed.
//! - It sends the DAP `process` event carrying `systemProcessId`, so the
//!   debuggee's pid arrives as data rather than scraped out of console text
//!   the way codelldb forces (quirk 9). The shared handshake reads it.
//! - It is the stricter of the two about sequencing: it sends no `initialized`
//!   event until it has received a `launch`. The handshake already writes
//!   `launch` without awaiting its response, which is the only order that
//!   works for either adapter.
//!
//! What it does need is care in the launch arguments — see
//! [`lazydap_dap::PythonLaunchArgs`], where `console`, `justMyCode` and
//! `subProcess` each carry the reason lazydap sends what it sends.

use super::{DebugAdapter, Spawn};
use lazydap_core::AdapterKind;
use lazydap_dap::PythonLaunchArgs;
use lazydap_protocol::LaunchRequest;
use std::path::Path;

/// The module that turns a Python interpreter into a debug adapter.
const ADAPTER_MODULE: &str = "debugpy.adapter";

pub struct Debugpy;

impl DebugAdapter for Debugpy {
    fn kind(&self) -> AdapterKind {
        AdapterKind::Debugpy
    }

    /// `<python> -m debugpy.adapter`, over the child's own stdin and stdout.
    ///
    /// `command` is a Python interpreter rather than an adapter binary, which
    /// is what discovery resolves for this kind. debugpy does install a
    /// `debugpy-adapter` script, but it is a generated shim that lands in a
    /// directory which is frequently not on `PATH` — a user-site install puts
    /// it in `~/Library/Python/3.14/bin` on macOS — while the interpreter that
    /// can import the module is on `PATH` by definition of having been found
    /// there.
    fn spawn(&self, command: &Path) -> Spawn {
        Spawn::Stdio {
            program: command.to_path_buf(),
            args: vec!["-m".into(), ADAPTER_MODULE.into()],
        }
    }

    fn adapter_id(&self) -> &'static str {
        "debugpy"
    }

    fn launch_args(&self, request: &LaunchRequest) -> serde_json::Value {
        let args = PythonLaunchArgs {
            adapter_type: "python".into(),
            request: "launch".into(),
            program: request.program.to_string_lossy().into_owned(),
            args: request.args.clone(),
            cwd: request.cwd.to_string_lossy().into_owned(),
            stop_on_entry: request.stop_on_entry,
            env: if request.env.is_empty() {
                None
            } else {
                Some(request.env.clone().into_iter().collect())
            },
            console: "internalConsole".into(),
            just_my_code: false,
            sub_process: false,
        };
        serde_json::to_value(args).expect("launch arguments are plain data")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn request() -> LaunchRequest {
        LaunchRequest {
            adapter: AdapterKind::Debugpy,
            program: PathBuf::from("/tmp/main.py"),
            args: vec!["--fast".into()],
            cwd: PathBuf::from("/tmp"),
            env: BTreeMap::new(),
            stop_on_entry: true,
            adapter_command: None,
        }
    }

    #[test]
    fn the_debuggee_stays_attached_and_nothing_asks_for_a_terminal() {
        // Any console but `internalConsole` makes debugpy ask for a terminal
        // with a `runInTerminal` reverse request lazydap does not advertise,
        // and sends the debuggee's stdout somewhere we would never read it.
        let json = Debugpy.launch_args(&request()).to_string();

        assert!(
            json.contains(r#""console":"internalConsole""#),
            "got: {json}"
        );
        assert!(json.contains(r#""stopOnEntry":true"#), "got: {json}");
        assert!(json.contains(r#""args":["--fast"]"#), "got: {json}");
        assert!(json.contains(r#""type":"python""#), "got: {json}");
    }

    #[test]
    fn library_code_is_debuggable_and_subprocesses_are_not_followed() {
        // `justMyCode: false` because an agent's bug is as often in a
        // dependency as in the project; `subProcess: false` because following
        // one means a `startDebugging` reverse request asking for a second
        // session, and lazydap runs one at a time (D007).
        let json = Debugpy.launch_args(&request()).to_string();

        assert!(json.contains(r#""justMyCode":false"#), "got: {json}");
        assert!(json.contains(r#""subProcess":false"#), "got: {json}");
    }

    #[test]
    fn an_empty_environment_is_omitted_rather_than_sent_as_null() {
        let json = Debugpy.launch_args(&request()).to_string();
        assert!(!json.contains("env"), "got: {json}");
    }

    #[test]
    fn debugpy_is_a_module_of_an_interpreter_not_a_binary() {
        match Debugpy.spawn(Path::new("/opt/homebrew/bin/python3")) {
            Spawn::Stdio { program, args } => {
                assert_eq!(program, PathBuf::from("/opt/homebrew/bin/python3"));
                assert_eq!(args, vec!["-m".to_string(), "debugpy.adapter".to_string()]);
            }
            other => unreachable!("debugpy speaks over stdio, got: {other:?}"),
        }
    }

    #[test]
    fn an_entry_stop_needs_no_normalising() {
        // Verified against debugpy 1.8.21: a stop-on-entry launch reports
        // `reason: "entry"` natively, so codelldb's D033 renaming — which
        // exists because LLDB calls its own SIGSTOP an exception — has nothing
        // to do here, and nothing is hidden in `raw_reason`.
        let (reason, raw) = Debugpy.normalise_stop("entry", "", true);
        assert_eq!(reason, lazydap_core::PauseReason::Entry);
        assert_eq!(raw, None);
    }

    #[test]
    fn no_pid_is_scraped_out_of_console_text() {
        // debugpy sends the DAP `process` event, so the handshake reads the
        // pid from data rather than from a human-readable line.
        assert_eq!(
            Debugpy.debuggee_pid_in("Launched process 56254 from '/tmp/hello'"),
            None,
        );
    }
}
