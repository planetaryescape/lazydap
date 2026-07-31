//! delve: Go.
//!
//! The third adapter. Delve speaks DAP natively — `dlv dap` is a first-class
//! mode rather than a shim — and follows the specification more closely than
//! codelldb does: it reports a stop-on-entry stop as `reason: "entry"`, and it
//! sends the DAP `process` event carrying `systemProcessId`, so neither
//! [`DebugAdapter::normalise_stop`] nor [`DebugAdapter::debuggee_pid_in`] has
//! anything to do here.
//!
//! What it does need is two launch arguments that are not optional in practice
//! and one that is a choice:
//!
//! - **`outputMode: "remote"`**, without which the debuggee's stdout goes to
//!   the adapter's own stdout and never reaches DAP at all.
//! - **`mode`**, which says whether `program` is source to compile (`"debug"`)
//!   or a binary to run (`"exec"`). lazydap reads that off the filename.
//! - **`output`**, aiming `mode: "debug"`'s compiled binary at a temporary
//!   path instead of the daemon's working directory.
//!
//! All three, and the entry-stop stack that is not there, are written up in
//! `docs/reference/delve-quirks.md`. Every one of them was found by running
//! delve 1.27.0, not by reading its documentation.

use super::{DebugAdapter, Spawn};
use lazydap_core::AdapterKind;
use lazydap_dap::{AdapterStream, GoLaunchArgs, PortAnnouncement, TcpSpawn};
use lazydap_protocol::LaunchRequest;
use std::path::Path;

pub struct Delve;

/// What delve prints when its DAP server is up, immediately before the address.
///
/// The whole line is `DAP server listening at: 127.0.0.1:54421`, on **stdout**
/// — where codelldb uses stderr and different words.
const PORT_MARKER: &str = "listening at: ";

impl DebugAdapter for Delve {
    fn kind(&self) -> AdapterKind {
        AdapterKind::Delve
    }

    /// `dlv dap --listen=127.0.0.1:0`, over TCP.
    ///
    /// Port zero for the reason codelldb gets it (quirk 3): letting the
    /// operating system choose is what keeps two lazydap projects on one
    /// machine from fighting over a fixed one. `127.0.0.1` rather than a bare
    /// `:0` because a debug adapter that accepts connections from the network
    /// is a remote code execution service.
    fn spawn(&self, command: &Path) -> Spawn {
        Spawn::Tcp(TcpSpawn {
            program: command.to_path_buf(),
            args: vec!["dap".into(), "--listen=127.0.0.1:0".into()],
            env: Vec::new(),
            announcement: PortAnnouncement {
                stream: AdapterStream::Stdout,
                marker: PORT_MARKER,
            },
        })
    }

    fn adapter_id(&self) -> &'static str {
        "go"
    }

    fn launch_args(&self, request: &LaunchRequest) -> serde_json::Value {
        let mode = launch_mode(&request.program);
        let args = GoLaunchArgs {
            adapter_type: "go".into(),
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
            output_mode: "remote".into(),
            output: (mode == DEBUG).then(compiled_binary_path),
            mode: mode.into(),
        };
        serde_json::to_value(args).expect("launch arguments are plain data")
    }
}

/// Compile the program, then run it.
const DEBUG: &str = "debug";
/// Run the program as it is.
const EXEC: &str = "exec";

/// Whether delve should build `program` or just run it.
///
/// Read off the extension, which is the same thing
/// [`AdapterKind::for_program`] reads and means the same thing here: a `.go`
/// file is source, and anything else a caller has aimed at delve is a binary
/// they have already built. Getting this wrong is not subtle — delve rejects a
/// `.go` file in `exec` mode and a binary in `debug` mode, both with a message
/// naming the file.
fn launch_mode(program: &Path) -> &'static str {
    match program.extension().and_then(|extension| extension.to_str()) {
        Some("go") => DEBUG,
        _ => EXEC,
    }
}

/// Where `mode: "debug"` should leave the binary it compiles.
///
/// Unique per launch: delve removes this itself on `disconnect`, but a name
/// shared between launches would let one session's cleanup delete the file
/// another was about to run.
fn compiled_binary_path() -> String {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir()
        .join(format!("lazydap-delve-{}-{unique}", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn request(program: &str) -> LaunchRequest {
        LaunchRequest {
            adapter: AdapterKind::Delve,
            program: PathBuf::from(program),
            args: vec!["--fast".into()],
            cwd: PathBuf::from("/tmp"),
            env: BTreeMap::new(),
            stop_on_entry: true,
            adapter_command: None,
        }
    }

    #[test]
    fn the_debuggee_s_output_is_asked_for_over_dap_rather_than_left_on_the_adapter_s_stdout() {
        // Quirk 2, and the single most consequential line in this file: delve's
        // default sends the debuggee's stdout to its own, where nothing reads
        // it, and every `continue --wait` blob comes back with an empty
        // `captured_output` while the program is visibly printing.
        let json = Delve.launch_args(&request("/tmp/main.go")).to_string();

        assert!(json.contains(r#""outputMode":"remote""#), "got: {json}");
        assert!(json.contains(r#""stopOnEntry":true"#), "got: {json}");
        assert!(json.contains(r#""args":["--fast"]"#), "got: {json}");
        assert!(json.contains(r#""type":"go""#), "got: {json}");
    }

    #[test]
    fn a_go_source_file_is_compiled_and_a_binary_is_not() {
        assert!(
            Delve
                .launch_args(&request("/tmp/main.go"))
                .to_string()
                .contains(r#""mode":"debug""#),
        );
        assert!(
            Delve
                .launch_args(&request("/tmp/hello"))
                .to_string()
                .contains(r#""mode":"exec""#),
        );
    }

    #[test]
    fn the_compiled_binary_is_aimed_at_a_temporary_directory_not_the_daemon_s_cwd() {
        // Quirk 4: delve's default is `__debug_bin<random>` in its own working
        // directory — the daemon's, which is a user's repository.
        let json = Delve.launch_args(&request("/tmp/main.go"));
        let output = json["output"].as_str().expect("debug mode names an output");

        assert!(
            output.starts_with(&std::env::temp_dir().to_string_lossy().into_owned()),
            "got: {output}",
        );
        assert!(
            !Delve
                .launch_args(&request("/tmp/hello"))
                .to_string()
                .contains("output\":\""),
            "an `exec` launch compiles nothing, so it has nowhere to put a binary",
        );
    }

    #[test]
    fn two_launches_do_not_share_a_compiled_binary() {
        // A shared name would let one session's cleanup delete the file
        // another was about to run.
        assert_ne!(compiled_binary_path(), compiled_binary_path());
    }

    #[test]
    fn an_empty_environment_is_omitted_rather_than_sent_as_null() {
        let json = Delve.launch_args(&request("/tmp/main.go")).to_string();
        assert!(!json.contains(r#""env""#), "got: {json}");
    }

    #[test]
    fn delve_is_reached_over_tcp_on_a_port_it_announces_on_stdout() {
        // codelldb announces on stderr, under different words, and needs
        // `RUST_LOG=debug` before it says anything at all. Nothing about the
        // two startups is shared.
        match Delve.spawn(Path::new("/usr/local/bin/dlv")) {
            Spawn::Tcp(spawn) => {
                assert_eq!(spawn.args, vec!["dap", "--listen=127.0.0.1:0"]);
                assert_eq!(spawn.announcement.stream, AdapterStream::Stdout);
                assert!(spawn.env.is_empty());
                assert!(
                    "DAP server listening at: 127.0.0.1:54421".contains(spawn.announcement.marker),
                    "the marker has to match what delve actually prints",
                );
            }
            other => unreachable!("delve speaks over TCP, got: {other:?}"),
        }
    }

    #[test]
    fn an_entry_stop_needs_no_normalising() {
        // Verified against delve 1.27.0: a stop-on-entry launch reports
        // `reason: "entry"` natively, so codelldb's D033 renaming has nothing
        // to do here.
        let (reason, raw) = Delve.normalise_stop("entry", "", true);
        assert_eq!(reason, lazydap_core::PauseReason::Entry);
        assert_eq!(raw, None);
    }

    #[test]
    fn no_pid_is_scraped_out_of_console_text() {
        // delve sends the DAP `process` event, so the handshake reads the pid
        // from data rather than from a human-readable line.
        assert_eq!(
            Delve.debuggee_pid_in("Launched process 56254 from '/tmp/hello'"),
            None,
        );
    }
}
