//! The auto-spawn lifecycle, driven through the real `lazydap` binary.
//!
//! Each test gets its own instance name and its own runtime and data
//! directories, so it never touches the developer's actual daemon. No
//! debuggee is launched: this is about the daemon coming up, staying up, and
//! going away when told.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const LAZYDAP: &str = env!("CARGO_BIN_EXE_lazydap");

/// An isolated instance, shut down and deleted when the test ends.
struct Sandbox {
    root: PathBuf,
    instance: String,
}

impl Sandbox {
    /// Names are kept terse on purpose. A Unix socket path has about a hundred
    /// bytes to play with, and `lazydap` refuses to bind one that overruns it;
    /// a chatty test directory is the easiest way to trip that for no reason.
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let instance = format!("{label}{}-{unique}", std::process::id());
        let root = PathBuf::from("/tmp").join(format!("lzd-{instance}"));
        std::fs::create_dir_all(root.join("r")).expect("create the runtime directory");
        std::fs::create_dir_all(root.join("d")).expect("create the data directory");

        Self { root, instance }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(LAZYDAP)
            .env("LAZYDAP_INSTANCE", &self.instance)
            .env("LAZYDAP_RUNTIME_DIR", self.root.join("r"))
            .env("LAZYDAP_DATA_DIR", self.root.join("d"))
            .args(args)
            .output()
            .expect("run lazydap")
    }

    /// Run a command and parse its JSON, failing loudly with stderr attached —
    /// a daemon that would not start is the interesting part of the failure.
    fn json(&self, args: &[&str]) -> serde_json::Value {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "`lazydap {}` failed ({}): {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr),
        );
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        match serde_json::from_str(&stdout) {
            Ok(value) => value,
            Err(error) => unreachable!(
                "`lazydap {}` printed something that is not JSON ({error}): {stdout}",
                args.join(" "),
            ),
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = self.run(&["shutdown"]);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn the_first_command_starts_a_daemon_and_the_next_one_reuses_it() {
    let sandbox = Sandbox::new("auto");

    let first = sandbox.json(&["--format", "json", "status"]);
    assert_eq!(first["instance"], sandbox.instance);
    assert!(first["session"].is_null(), "nothing has been launched");
    let pid = first["daemon_pid"].as_u64().expect("a daemon pid");
    assert!(pid > 0);

    let second = sandbox.json(&["--format", "json", "status"]);
    assert_eq!(
        second["daemon_pid"].as_u64(),
        Some(pid),
        "the second command must reuse the daemon, not start another",
    );
    assert!(
        second["uptime_ms"].as_u64() >= first["uptime_ms"].as_u64(),
        "the same daemon should have been up for longer by now",
    );
}

#[test]
fn shutdown_stops_the_daemon_and_removes_its_socket() {
    let sandbox = Sandbox::new("down");

    let started = sandbox.json(&["--format", "json", "status"]);
    let socket = sandbox
        .root
        .join("r")
        .join(format!("lazydap-{}.sock", sandbox.instance));
    assert!(socket.exists(), "the daemon should have bound its socket");

    let stopped = sandbox.json(&["--format", "json", "shutdown"]);
    assert_eq!(stopped["shutting_down"], true);

    // Teardown is asynchronous on the daemon's side; give it a moment to
    // unlink rather than asserting on a race.
    for _ in 0..50 {
        if !socket.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        !socket.exists(),
        "a stopped daemon must not leave its socket"
    );

    // And the next command starts a fresh one.
    let restarted = sandbox.json(&["--format", "json", "status"]);
    assert_ne!(
        restarted["daemon_pid"], started["daemon_pid"],
        "the restarted daemon should be a different process",
    );
}

#[test]
fn shutting_down_when_nothing_is_running_is_not_an_error() {
    let sandbox = Sandbox::new("idle");

    let output = sandbox.json(&["--format", "json", "shutdown"]);
    assert_eq!(output["shutting_down"], false);
    assert_eq!(output["reason"], "no daemon was running");
}

#[test]
fn disconnecting_with_no_session_reports_that_rather_than_hanging() {
    let sandbox = Sandbox::new("nose");

    let output = sandbox.run(&["--format", "json", "disconnect"]);
    assert_eq!(output.status.code(), Some(1), "a general failure");

    let error: serde_json::Value =
        serde_json::from_slice(&output.stderr).expect("errors are JSON on stderr in JSON mode");
    assert_eq!(error["error"], "SessionNotFound");
}

#[test]
fn an_unknown_subcommand_is_a_usage_error() {
    let sandbox = Sandbox::new("use");

    let output = sandbox.run(&["explode"]);
    assert_eq!(output.status.code(), Some(2), "usage errors exit 2");
}
