//! codelldb: C, C++, Rust and anything else with DWARF.
//!
//! Everything in here is something codelldb does that the DAP specification
//! does not require, and that the shared handshake therefore must not assume.
//! It reaches its adapter over TCP rather than stdio, it implements
//! stop-on-entry with a signal, and it never sends the `process` event that
//! would say which process it started. See `docs/reference/codelldb-quirks.md`.

use super::{DebugAdapter, Spawn, StopContext};
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
            // Load the Rust and C/C++ LLDB formatters. Without this codelldb
            // renders a Rust `&str`/`String`/`Vec` as a raw pointer past its
            // length (quirk 10). codelldb ignores names for languages it has no
            // formatters for, so listing all three is safe for any LLDB debuggee.
            source_languages: Some(vec!["rust".into(), "cpp".into(), "c".into()]),
        };
        serde_json::to_value(args).expect("launch arguments are plain data")
    }

    /// What lazydap calls a stop, and what codelldb called it if those differ.
    ///
    /// codelldb stops a program by sending it `SIGSTOP`, and LLDB classifies a
    /// signal stop as an exception — so both of the two ways lazydap asks a
    /// program to stop on purpose report `reason: "exception"`
    /// (`docs/reference/codelldb-quirks.md`, quirk 6). An agent reading that
    /// concludes the program crashed.
    ///
    /// The two are told apart by what was asked for, because nothing in the
    /// stop itself distinguishes them:
    ///
    /// - a `pause` is outstanding — the stop is that pause, and is `pause`
    ///   (D064);
    /// - a `--stop-on-entry` launch has not stopped yet — the stop is the entry
    ///   point, and is `entry` (D033).
    ///
    /// Either way the adapter's own word is kept in `raw_reason`, so the
    /// normalisation is visible rather than a quiet substitution. The guard is
    /// deliberately narrow: a real exception does not carry `SIGSTOP`, a
    /// `SIGSTOP` nobody asked for passes through, and every other stop passes
    /// through untouched.
    fn normalise_stop(
        &self,
        raw: &str,
        description: &str,
        context: StopContext,
    ) -> (PauseReason, Option<String>) {
        let reason = PauseReason::from(raw);
        let is_stop_signal =
            matches!(reason, PauseReason::Exception) && description.contains("SIGSTOP");
        if !is_stop_signal {
            return (reason, None);
        }

        // Pause first: a launch's entry stop happens before there is a session
        // to ask for a pause, so the two cannot both be true — and if they ever
        // were, the request still outstanding is the better answer.
        let renamed = if context.pause_requested {
            PauseReason::Pause
        } else if context.stop_on_entry {
            PauseReason::Entry
        } else {
            return (reason, None);
        };
        tracing::debug!(
            target: "daemon.session",
            raw_reason = raw,
            description,
            %renamed,
            "reporting codelldb's SIGSTOP stop as what was asked for (quirk 6)",
        );
        (renamed, Some(raw.to_string()))
    }

    /// codelldb reports a failed expression as a *successful* `evaluate` whose
    /// result is the error text, so the DAP envelope says nothing is wrong
    /// (quirk 11, D068). Two shapes have been seen live:
    ///
    /// ```text
    /// <error: invalid value object>
    /// <read memory from 0x4 failed (0 of 4 bytes read)>
    /// ```
    ///
    /// Both are wrapped in angle brackets, and so is plenty of legitimate
    /// output — Python's `<__main__.Foo object at 0x10a>`, LLDB's own
    /// `<incomplete type>`. The brackets alone are therefore not enough: the
    /// text inside must also open with `error:` or say something `failed`.
    /// A value that only *contains* those words is left alone, and this runs
    /// for codelldb and nothing else.
    fn is_eval_error(&self, value: &str) -> bool {
        let Some(inner) = value
            .strip_prefix('<')
            .and_then(|value| value.strip_suffix('>'))
        else {
            return false;
        };
        inner.starts_with("error:") || inner.contains(" failed")
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
    fn launch_arguments_load_the_rust_and_c_type_formatters() {
        // Without `sourceLanguages`, codelldb renders a Rust `&str`/`String`/
        // `Vec` as a raw pointer and dumps rodata past the slice length
        // (quirk 10, found dogfooding lazydap on its own Rust binary). Rust is
        // a target language, so this arg is not optional polish.
        let json = CodeLldb.launch_args(&request()).to_string();
        assert!(
            json.contains(r#""sourceLanguages":["rust","cpp","c"]"#),
            "got: {json}",
        );
    }

    fn entry_launch() -> StopContext {
        StopContext {
            stop_on_entry: true,
            pause_requested: false,
        }
    }

    fn after_a_pause() -> StopContext {
        StopContext {
            stop_on_entry: false,
            pause_requested: true,
        }
    }

    #[test]
    fn a_sigstop_entry_pause_is_reported_as_entry_with_the_adapter_s_word_kept() {
        // Quirk 6: this is what codelldb actually sends for a stop-on-entry
        // launch on macOS. An agent reading "exception" concludes the program
        // crashed before main.
        let (reason, raw) = CodeLldb.normalise_stop("exception", "signal SIGSTOP", entry_launch());
        assert_eq!(reason, PauseReason::Entry);
        assert_eq!(raw.as_deref(), Some("exception"), "nothing is hidden");
    }

    #[test]
    fn a_stop_the_caller_asked_for_with_pause_is_reported_as_pause() {
        // Verbatim from a live `lazydap pause --wait` against a spinning C
        // program: codelldb implements pause with the same SIGSTOP it uses for
        // stop-on-entry, so an agent reading "exception" concludes its own
        // pause was a crash (D064).
        let (reason, raw) = CodeLldb.normalise_stop("exception", "signal SIGSTOP", after_a_pause());
        assert_eq!(reason, PauseReason::Pause);
        assert_eq!(raw.as_deref(), Some("exception"), "nothing is hidden");
    }

    #[test]
    fn a_real_exception_during_a_pause_is_still_an_exception() {
        // The guard is the SIGSTOP signature, not the outstanding pause: a
        // program that segfaults while a pause is in flight has crashed.
        let (reason, raw) = CodeLldb.normalise_stop("exception", "EXC_BAD_ACCESS", after_a_pause());
        assert_eq!(reason, PauseReason::Exception);
        assert_eq!(raw, None);
    }

    #[test]
    fn a_real_exception_at_the_entry_point_is_still_an_exception() {
        let (reason, raw) = CodeLldb.normalise_stop("exception", "EXC_BAD_ACCESS", entry_launch());
        assert_eq!(reason, PauseReason::Exception);
        assert_eq!(raw, None, "nothing was renamed, so nothing to disclose");
    }

    #[test]
    fn a_sigstop_stop_nobody_asked_for_is_left_alone() {
        // Nobody asked for either a pause or an entry stop, so a SIGSTOP is
        // somebody else's doing and naming it would be an invention.
        let (reason, raw) =
            CodeLldb.normalise_stop("exception", "signal SIGSTOP", StopContext::default());
        assert_eq!(reason, PauseReason::Exception);
        assert_eq!(raw, None);
    }

    #[test]
    fn an_adapter_that_follows_the_spec_needs_no_normalising() {
        let (reason, raw) = CodeLldb.normalise_stop("entry", "", entry_launch());
        assert_eq!(reason, PauseReason::Entry);
        assert_eq!(raw, None);
    }

    #[test]
    fn an_error_codelldb_hid_inside_a_value_is_recognised_as_one() {
        // Both captured live: codelldb answers an expression it could not
        // evaluate with a *successful* `evaluate` whose result is the error
        // (D068).
        assert!(CodeLldb.is_eval_error("<error: invalid value object>"));
        assert!(CodeLldb.is_eval_error("<read memory from 0x4 failed (0 of 4 bytes read)>"));
    }

    #[test]
    fn a_value_that_merely_has_angle_brackets_is_still_a_value() {
        // The brackets alone cannot be the test. Getting this wrong turns a
        // working `eval` into a failure, which is worse than the bug.
        assert!(!CodeLldb.is_eval_error("42"));
        assert!(!CodeLldb.is_eval_error("<__main__.Foo object at 0x10a>"));
        assert!(!CodeLldb.is_eval_error("<incomplete type>"));
        assert!(!CodeLldb.is_eval_error("\"the last attempt failed\""));
        assert!(!CodeLldb.is_eval_error("<0 of 4 bytes read> failed"));
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
