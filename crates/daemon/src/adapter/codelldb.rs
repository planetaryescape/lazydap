//! codelldb: C, C++, Rust and anything else with DWARF.
//!
//! Everything in here is something codelldb does that the DAP specification
//! does not require, and that the shared handshake therefore must not assume.
//! It reaches its adapter over TCP rather than stdio, it implements
//! stop-on-entry with a signal, and it never sends the `process` event that
//! would say which process it started. See `docs/reference/codelldb-quirks.md`.

use super::{DebugAdapter, Spawn};
use lazydap_core::{AdapterKind, PauseReason};
use lazydap_dap::{AdapterStream, LaunchArgs, TcpSpawn};
use lazydap_protocol::LaunchRequest;
use std::path::Path;

pub struct CodeLldb;

impl DebugAdapter for CodeLldb {
    fn kind(&self) -> AdapterKind {
        AdapterKind::Codelldb
    }

    /// Over TCP. codelldb can speak DAP on stdio too, but only its `--port`
    /// mode reports the port it chose, and letting it choose is what keeps two
    /// lazydap projects on one machine from fighting over a fixed one
    /// (quirk 3).
    ///
    /// `RUST_LOG=debug` is not optional: codelldb logs its `Listening on ...`
    /// line at debug level, so without it the adapter is silent on stderr and
    /// the transport's line loop waits forever. See
    /// `docs/issues/0002-codelldb-version-drift-rust-log.md`.
    fn spawn(&self, command: &Path) -> Spawn {
        Spawn::Tcp(TcpSpawn {
            program: command.to_path_buf(),
            args: vec!["--port".into(), "0".into()],
            env: vec![("RUST_LOG".into(), "debug".into())],
            port_stream: AdapterStream::Stderr,
            port_marker: "Listening on ",
        })
    }

    fn adapter_id(&self) -> &'static str {
        "lldb"
    }

    fn launch_args(&self, request: &LaunchRequest) -> serde_json::Value {
        let args = LaunchArgs {
            adapter_type: "lldb".into(),
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
            // codelldb defaults to the integrated terminal, which needs a
            // runInTerminal reverse request we deliberately do not advertise.
            // "console" keeps the debuggee attached so its stdout arrives as
            // DAP output events.
            terminal: Some("console".into()),
        };
        serde_json::to_value(args).expect("launch arguments are plain data")
    }

    /// What lazydap calls a stop, and what codelldb called it if those differ.
    ///
    /// codelldb implements entry-stop by letting the process start and sending
    /// it `SIGSTOP`; LLDB classifies a signal stop as an exception, so a launch
    /// that did exactly what was asked reports `reason: "exception"`
    /// (`docs/reference/codelldb-quirks.md`, quirk 6). An agent reading that
    /// concludes the program crashed before `main`.
    ///
    /// So the first stop of a `--stop-on-entry` launch, and only that one, is
    /// renamed to `entry` — and the adapter's own word is kept in `raw_reason`,
    /// so the normalisation is visible rather than a quiet substitution (D033).
    /// The guard is deliberately narrow: a real exception at the entry point
    /// would not carry `SIGSTOP`, and every later stop passes through
    /// untouched.
    fn normalise_stop(
        &self,
        raw: &str,
        description: &str,
        stop_on_entry: bool,
    ) -> (PauseReason, Option<String>) {
        let reason = PauseReason::from(raw);
        let is_entry_signal =
            matches!(reason, PauseReason::Exception) && description.contains("SIGSTOP");

        if stop_on_entry && is_entry_signal {
            tracing::debug!(
                target: "daemon.session",
                raw_reason = raw,
                description,
                "reporting codelldb's SIGSTOP entry stop as `entry` (quirk 6)",
            );
            return (PauseReason::Entry, Some(raw.to_string()));
        }
        (reason, None)
    }

    /// The debuggee's pid, scraped from the line codelldb prints when it starts
    /// one.
    ///
    /// DAP has a `process` event carrying `systemProcessId` and this is what it
    /// is for. codelldb does not send it — the string is not in its binary, and
    /// a full launch-to-exit stream carries `output`, `initialized`, `module`,
    /// `continued`, `exited` and `terminated` and nothing else. What it does
    /// print, to the console category, is:
    ///
    /// ```text
    /// Launched process 56254 from '/path/to/program'
    /// ```
    ///
    /// So that is where the pid comes from (quirk 9). Scraping a human-readable
    /// line is exactly as brittle as it looks, which is why every caller treats
    /// a `None` as "carry on without it": the only thing it costs is the
    /// best-effort cleanup in [`crate::debuggee`]. debugpy, by contrast, sends
    /// the event and needs none of this.
    fn debuggee_pid_in(&self, output: &str) -> Option<u32> {
        let rest = output.strip_prefix("Launched process ")?;
        let (pid, _) = rest.split_once(char::is_whitespace)?;
        pid.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazydap_core::AdapterKind;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn request() -> LaunchRequest {
        LaunchRequest {
            adapter: AdapterKind::Codelldb,
            program: PathBuf::from("/tmp/hello"),
            args: vec!["--fast".into()],
            cwd: PathBuf::from("/tmp"),
            env: BTreeMap::new(),
            stop_on_entry: true,
            adapter_command: None,
        }
    }

    #[test]
    fn launch_arguments_keep_the_debuggee_attached_to_the_adapter() {
        let json = CodeLldb.launch_args(&request()).to_string();

        assert!(json.contains(r#""terminal":"console""#), "got: {json}");
        assert!(json.contains(r#""stopOnEntry":true"#), "got: {json}");
        assert!(json.contains(r#""args":["--fast"]"#), "got: {json}");
        assert!(
            !json.contains("env"),
            "an empty environment must be omitted, not sent as null: {json}",
        );
    }

    #[test]
    fn a_sigstop_entry_pause_is_reported_as_entry_with_the_adapter_s_word_kept() {
        // Quirk 6: this is what codelldb actually sends for a stop-on-entry
        // launch on macOS. An agent reading "exception" concludes the program
        // crashed before main.
        let (reason, raw) = CodeLldb.normalise_stop("exception", "signal SIGSTOP", true);
        assert_eq!(reason, PauseReason::Entry);
        assert_eq!(raw.as_deref(), Some("exception"), "nothing is hidden");
    }

    #[test]
    fn a_real_exception_at_the_entry_point_is_still_an_exception() {
        let (reason, raw) = CodeLldb.normalise_stop("exception", "EXC_BAD_ACCESS", true);
        assert_eq!(reason, PauseReason::Exception);
        assert_eq!(raw, None, "nothing was renamed, so nothing to disclose");
    }

    #[test]
    fn a_sigstop_stop_nobody_asked_for_is_left_alone() {
        // Without `--stop-on-entry` a SIGSTOP is somebody else's doing, and
        // calling it an entry stop would be an invention.
        let (reason, raw) = CodeLldb.normalise_stop("exception", "signal SIGSTOP", false);
        assert_eq!(reason, PauseReason::Exception);
        assert_eq!(raw, None);
    }

    #[test]
    fn an_adapter_that_follows_the_spec_needs_no_normalising() {
        let (reason, raw) = CodeLldb.normalise_stop("entry", "", true);
        assert_eq!(reason, PauseReason::Entry);
        assert_eq!(raw, None);
    }

    #[test]
    fn the_pid_is_scraped_from_the_line_codelldb_prints() {
        assert_eq!(
            CodeLldb.debuggee_pid_in("Launched process 56254 from '/tmp/hello'"),
            Some(56254),
        );
        assert_eq!(CodeLldb.debuggee_pid_in("hello from the debuggee\n"), None);
    }

    #[test]
    fn codelldb_is_reached_over_tcp() {
        assert!(matches!(
            CodeLldb.spawn(Path::new("/opt/codelldb/codelldb")),
            Spawn::Tcp { .. },
        ));
    }
}
