//! The auto-spawn lifecycle, driven through the real `lazydap` binary.
//!
//! Each test gets its own instance name and its own runtime and data
//! directories, so it never touches the developer's actual daemon. No
//! debuggee is launched: this is about the daemon coming up, staying up, and
//! going away when told.

use std::os::unix::fs::PermissionsExt;
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
        self.run_with_config(None, args)
    }

    /// The project directory commands that touch `.lazydap/state.toml` run in.
    ///
    /// It carries a `.lazydap` marker so [`project_root`] stops here rather
    /// than walking up. Without it a `lazydap watch add` run from the test
    /// harness resolves the *repository* as its project and writes the
    /// developer's own state file.
    ///
    /// [`project_root`]: lazydap_config::paths::project_root
    fn project(&self) -> PathBuf {
        let project = self.root.join("p");
        std::fs::create_dir_all(project.join(".lazydap")).expect("create the project root");
        project
    }

    /// Run inside [`Self::project`], which is what makes project state land in
    /// the sandbox. The daemon inherits this working directory when the first
    /// command spawns it, and resolves its store from there.
    fn run_in_project(&self, args: &[&str]) -> Output {
        self.run_in_project_with_env(&[], args)
    }

    /// The same, with extra environment: a `PATH` an adapter is missing from,
    /// a `LAZYDAP_TIMEOUT` nothing can read, a `LAZYDAP_CONFIG_PATH`.
    fn run_in_project_with_env(&self, env: &[(&str, &str)], args: &[&str]) -> Output {
        let mut command = Command::new(LAZYDAP);
        command
            .current_dir(self.project())
            .env("LAZYDAP_INSTANCE", &self.instance)
            .env("LAZYDAP_RUNTIME_DIR", self.root.join("r"))
            .env("LAZYDAP_DATA_DIR", self.root.join("d"));
        for (key, value) in env {
            command.env(key, value);
        }
        command.args(args).output().expect("run lazydap")
    }

    /// Write a file inside the project, creating the directories above it.
    fn write_in_project(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.project().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create the directory");
        }
        std::fs::write(&path, body).expect("write the file");
        path
    }

    /// Everything the daemon would leave behind if one had been started.
    fn daemon_files(&self) -> Vec<String> {
        ["r", "d"]
            .iter()
            .flat_map(|dir| std::fs::read_dir(self.root.join(dir)).expect("read the directory"))
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    /// [`Self::run_in_project`] plus the JSON parse and the loud failure.
    fn json_in_project(&self, args: &[&str]) -> serde_json::Value {
        let output = self.run_in_project(args);
        assert!(
            output.status.success(),
            "`lazydap {}` failed ({}): {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr),
        );
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        serde_json::from_str(&stdout).unwrap_or_else(|error| {
            unreachable!(
                "`lazydap {}` printed something that is not JSON ({error}): {stdout}",
                args.join(" "),
            )
        })
    }

    fn stdout(&self, args: &[&str]) -> String {
        let output = self.run_in_project(args);
        assert!(
            output.status.success(),
            "`lazydap {}` failed ({}): {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// The same, with `LAZYDAP_CONFIG_PATH` pointed somewhere — including at a
    /// file that is not valid TOML.
    fn run_with_config(&self, config: Option<&PathBuf>, args: &[&str]) -> Output {
        let mut command = Command::new(LAZYDAP);
        command
            .env("LAZYDAP_INSTANCE", &self.instance)
            .env("LAZYDAP_RUNTIME_DIR", self.root.join("r"))
            .env("LAZYDAP_DATA_DIR", self.root.join("d"));
        if let Some(config) = config {
            command.env("LAZYDAP_CONFIG_PATH", config);
        }
        command.args(args).output().expect("run lazydap")
    }

    /// Write a config file inside the sandbox and return its path.
    fn write_config(&self, body: &str) -> PathBuf {
        let path = self.root.join("config.toml");
        std::fs::write(&path, body).expect("write the config");
        path
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
fn a_broken_config_does_not_take_the_recovery_commands_with_it() {
    // The moment this matters: a debuggee is running, something is wrong, and
    // the config file has a typo in it. `status` and `shutdown` are what you
    // reach for, and they have to work.
    let sandbox = Sandbox::new("cfgbad");
    let config = sandbox.write_config("[general\nwait_timeout_seconds = ");

    let output = sandbox.run_with_config(Some(&config), &["--format", "json", "status"]);

    assert!(
        output.status.success(),
        "status must survive a config it cannot parse: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("status still prints its JSON");
    assert_eq!(report["instance"], sandbox.instance);

    let shutdown = sandbox.run_with_config(Some(&config), &["--format", "json", "shutdown"]);
    assert!(shutdown.status.success(), "and so must shutdown");
}

#[test]
fn a_broken_state_file_fails_fast_and_leaves_no_socket_behind() {
    // `.lazydap/state.toml` is documented as hand-editable (D006), so a typo
    // in it is an ordinary way for the daemon to refuse to start. It used to
    // bind its socket first and die after, which cost every later command the
    // client's full ten-second spawn deadline and told it nothing.
    let sandbox = Sandbox::new("statebad");
    std::fs::write(
        sandbox.project().join(".lazydap").join("state.toml"),
        "[[breakpoints\nbroken",
    )
    .expect("write a malformed state file");

    let started = std::time::Instant::now();
    let output = sandbox.run_in_project(&["--format", "json", "status"]);
    let elapsed = started.elapsed();

    assert!(!output.status.success(), "a daemon that cannot start fails");
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "it must not wait out the spawn deadline; took {elapsed:?}",
    );

    let error: serde_json::Value =
        serde_json::from_slice(&output.stderr).expect("errors are JSON on stderr");
    let message = error["message"].as_str().expect("a message");
    assert!(
        message.contains("state.toml") && message.contains("TOML parse error"),
        "the message must name the real problem: {message}",
    );

    // The socket lives in the runtime directory and the pid file in the data
    // one; neither should exist.
    let leftovers: Vec<String> = ["r", "d"]
        .iter()
        .flat_map(|dir| std::fs::read_dir(sandbox.root.join(dir)).expect("read the directory"))
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".sock") || name.ends_with(".pid"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "a daemon that never served must leave nothing to connect to: {leftovers:?}",
    );
}

#[test]
fn three_commands_racing_a_broken_state_file_all_fail_fast() {
    // Only one of them wins the spawn lock. The losers used to wait out the
    // full ten-second deadline for a daemon nobody was starting any more, and
    // report the connection refusal rather than the reason.
    let sandbox = Sandbox::new("statebadrace");
    std::fs::write(
        sandbox.project().join(".lazydap").join("state.toml"),
        "[[breakpoints\nbroken",
    )
    .expect("write a malformed state file");

    let started = std::time::Instant::now();
    let outputs: Vec<Output> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..3)
            .map(|_| scope.spawn(|| sandbox.run_in_project(&["--format", "json", "status"])))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("the command ran"))
            .collect()
    });
    let elapsed = started.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "losing the spawn race must not cost the whole deadline; took {elapsed:?}",
    );
    for output in &outputs {
        assert!(!output.status.success(), "each one fails");
        let error: serde_json::Value =
            serde_json::from_slice(&output.stderr).expect("errors are JSON on stderr");
        let message = error["message"].as_str().expect("a message");
        assert!(
            message.contains("TOML parse error"),
            "every racer must be told why, not just that nothing answered: {message}",
        );
    }
}

#[test]
fn doctor_reports_a_broken_config_rather_than_dying_of_it() {
    let sandbox = Sandbox::new("cfgdoc");
    let config = sandbox.write_config("[general\nwait_timeout_seconds = ");

    let output = sandbox.run_with_config(Some(&config), &["--format", "json", "doctor"]);

    // A failed check is a failed command — but the report still prints, which
    // is the whole point of running doctor.
    assert_eq!(output.status.code(), Some(1), "a failed check fails");
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the report is still on stdout");
    let config_check = report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["name"] == "config.file")
        .expect("doctor must say something about the config file");

    assert_eq!(config_check["ok"], false);
    let detail = config_check["detail"].as_str().expect("a detail");
    assert!(detail.contains("config.toml"), "the path: {detail}");
    assert_eq!(report["ok"], false);
}

#[test]
fn a_launch_refuses_outright_when_the_config_cannot_be_read() {
    // The other half of the bargain: the adapter comes from this file, so a
    // command that would act on it does not guess.
    let sandbox = Sandbox::new("cfglau");
    let config = sandbox.write_config("[adapter.codelldb\ncommand = ");

    let output = sandbox.run_with_config(Some(&config), &["--format", "json", "launch", "./x"]);

    assert_eq!(output.status.code(), Some(1), "it must not launch");
    let error: serde_json::Value =
        serde_json::from_slice(&output.stderr).expect("errors are JSON on stderr");
    assert_eq!(error["error"], "InvalidLaunchConfig");
}

#[test]
fn an_unknown_subcommand_is_a_usage_error() {
    let sandbox = Sandbox::new("use");

    let output = sandbox.run(&["explode"]);
    assert_eq!(output.status.code(), Some(2), "usage errors exit 2");
}

// --- Watches (M16) ----------------------------------------------------------
//
// The CLI half of non-negotiable #2: every watch the TUI's pane can set, these
// commands can set, through the same requests. They also prove the thing the
// pane depends on — that an expression is project state, recorded without a
// session and still there after the daemon that recorded it has gone.

#[test]
fn a_watch_added_without_a_session_is_listed_back_and_can_be_removed() {
    let sandbox = Sandbox::new("wadd");

    let added = sandbox.json_in_project(&[
        "--format",
        "json",
        "watch",
        "add",
        "tokens[pos]",
        "--label",
        "token",
    ]);
    assert_eq!(added["action"], "added");
    assert_eq!(added["watches"][0]["expression"], "tokens[pos]");
    assert_eq!(added["watches"][0]["label"], "token");

    let listed = sandbox.json_in_project(&["--format", "json", "watch", "list"]);
    assert_eq!(listed["action"], "listed");
    assert_eq!(
        listed["watches"].as_array().expect("an array").len(),
        1,
        "no session was ever launched, and the watch is still recorded",
    );

    let removed = sandbox.json_in_project(&["--format", "json", "watch", "remove", "tokens[pos]"]);
    assert_eq!(removed["action"], "removed");
    assert_eq!(removed["watches"][0]["expression"], "tokens[pos]");

    let empty = sandbox.json_in_project(&["--format", "json", "watch", "list"]);
    assert!(
        empty["watches"].as_array().expect("an array").is_empty(),
        "got: {empty}",
    );
}

#[test]
fn watch_list_in_the_ids_format_feeds_a_removal_by_id() {
    // The composability claim `--format ids` exists for:
    //   lazydap watch list --format ids | xargs -I{} lazydap watch remove --id {}
    let sandbox = Sandbox::new("wids");
    for expression in ["a", "b", "c"] {
        sandbox.run_in_project(&["watch", "add", expression]);
    }

    let ids: Vec<String> = sandbox
        .stdout(&["--format", "ids", "watch", "list"])
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(ids, vec!["1", "2", "3"], "one bare id per line");

    let removed = sandbox.json_in_project(&[
        "--format", "json", "watch", "remove", "--id", &ids[1], "--id", &ids[2],
    ]);
    assert_eq!(removed["watches"].as_array().expect("an array").len(), 2);

    let left = sandbox.json_in_project(&["--format", "json", "watch", "list"]);
    assert_eq!(left["watches"][0]["expression"], "a");
}

#[test]
fn a_dry_run_watch_command_changes_nothing() {
    // Non-negotiable #4, and the preview picks the same watches the real
    // removal does because both go through `store.select_watches`.
    let sandbox = Sandbox::new("wdry");
    sandbox.run_in_project(&["watch", "add", "counter"]);

    let preview =
        sandbox.json_in_project(&["--format", "json", "watch", "add", "other", "--dry-run"]);
    assert_eq!(preview["dry_run"], true);
    assert_eq!(
        preview["watches"][0]["id"], 0,
        "a preview does not promise an id it has not allocated",
    );

    let preview = sandbox.json_in_project(&[
        "--format",
        "json",
        "watch",
        "remove",
        "counter",
        "--dry-run",
    ]);
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["watches"][0]["expression"], "counter");

    let still_there = sandbox.json_in_project(&["--format", "json", "watch", "list"]);
    let watches = still_there["watches"].as_array().expect("an array");
    assert_eq!(watches.len(), 1, "neither preview changed anything");
    assert_eq!(watches[0]["expression"], "counter");
}

#[test]
fn a_watch_survives_the_daemon_it_was_added_through() {
    // The expressions are the project's, not the daemon's. This is the CLI
    // half of the TUI evidence that a watch outlives a disconnect.
    let sandbox = Sandbox::new("wpersist");
    sandbox.run_in_project(&["watch", "add", "counter"]);
    sandbox.run_in_project(&["shutdown"]);

    let listed = sandbox.json_in_project(&["--format", "json", "watch", "list"]);
    assert_eq!(
        listed["watches"][0]["expression"], "counter",
        "a new daemon read it back off disk: {listed}",
    );
}

#[test]
fn removing_a_watch_two_ways_at_once_is_a_usage_error() {
    // The two readings remove different watches, so it refuses rather than
    // guessing — and it refuses before starting a daemon.
    let sandbox = Sandbox::new("wamb");

    let output = sandbox.run_in_project(&["watch", "remove", "x", "--all"]);
    assert_eq!(output.status.code(), Some(2), "usage errors exit 2");
}

// --- Breakpoints ------------------------------------------------------------

#[test]
fn re_setting_a_location_updates_it_and_says_so() {
    // The defect: the modifiers were dropped on the floor and the command
    // still reported `added`, so a script that set a condition on a line it
    // had already broken on debugged with an unconditional breakpoint and no
    // sign anything had gone wrong (D086).
    let sandbox = Sandbox::new("bpupd");
    let source = sandbox.project().join("f.c");
    std::fs::write(&source, "int main(void) { return 0; }\n").expect("write the fixture");
    let location = format!("{}:1", source.display());

    let added = sandbox.json_in_project(&["--format", "json", "break", &location]);
    assert_eq!(added["action"], "added", "got: {added}");
    let id = added["breakpoints"][0]["id"].clone();

    let preview = sandbox.json_in_project(&[
        "--format",
        "json",
        "break",
        &location,
        "--condition",
        "i > 5",
        "--disabled",
        "--dry-run",
    ]);
    let updated = sandbox.json_in_project(&[
        "--format",
        "json",
        "break",
        &location,
        "--condition",
        "i > 5",
        "--disabled",
    ]);

    for report in [&preview, &updated] {
        assert_eq!(report["action"], "updated", "got: {report}");
        assert_eq!(report["breakpoints"][0]["id"], id, "got: {report}");
        assert_eq!(
            report["breakpoints"][0]["condition"], "i > 5",
            "got: {report}"
        );
        assert_eq!(report["breakpoints"][0]["enabled"], false, "got: {report}");
    }

    let listed = sandbox.json_in_project(&["--format", "json", "break", "--list"]);
    let breakpoints = listed["breakpoints"].as_array().expect("an array");
    assert_eq!(breakpoints.len(), 1, "one line, one breakpoint: {listed}");
    assert_eq!(breakpoints[0]["condition"], "i > 5", "got: {listed}");

    let again = sandbox.json_in_project(&[
        "--format",
        "json",
        "break",
        &location,
        "--condition",
        "i > 5",
        "--disabled",
    ]);
    assert_eq!(
        again["action"], "unchanged",
        "asking for what is already there changed nothing: {again}",
    );
}

// --- The CLI surface (WP5) --------------------------------------------------

#[test]
fn a_program_is_resolved_against_the_shell_not_against_cwd() {
    // `--cwd` says where the debuggee runs. `./app` on the command line means
    // the `./app` the person typing it can see, and resolving it against
    // `--cwd` made `lazydap launch ./app --cwd sub` fail for a program plainly
    // there — or, when `sub/app` existed too, debug the wrong binary in
    // silence.
    let sandbox = Sandbox::new("cwd");
    sandbox.write_in_project("sub/app", "#!/bin/sh\n");
    // A config pinning codelldb at nothing, so the launch stops at the next
    // step after the program resolves rather than starting an adapter.
    let config = sandbox.write_config("[adapter.codelldb]\ncommand = \"/nowhere/codelldb\"\n");
    let config = config.to_string_lossy().into_owned();
    let env = [("LAZYDAP_CONFIG_PATH", config.as_str())];

    let missing = sandbox.run_in_project_with_env(
        &env,
        &["--format", "json", "launch", "./app", "--cwd", "sub"],
    );
    assert_eq!(missing.status.code(), Some(1), "there is no ./app here");
    let error: serde_json::Value =
        serde_json::from_slice(&missing.stderr).expect("errors are JSON on stderr");
    assert_eq!(error["error"], "InvalidLaunchConfig");
    assert!(
        error["message"]
            .as_str()
            .expect("a message")
            .contains("cannot debug ./app"),
        "it must not have looked in sub/: {error}",
    );

    // And the same program, now where the shell is, resolves — the launch gets
    // as far as the adapter, which is the step after this one.
    sandbox.write_in_project("app", "#!/bin/sh\n");
    let found = sandbox.run_in_project_with_env(
        &env,
        &["--format", "json", "launch", "./app", "--cwd", "sub"],
    );
    assert_eq!(
        found.status.code(),
        Some(4),
        "expected the missing adapter, which is past the program: {}",
        String::from_utf8_lossy(&found.stderr),
    );
}

#[test]
fn logs_follow_refuses_the_formats_it_cannot_honour() {
    // It used to print the JSON object and then append bare log lines after
    // it, which no parser survives.
    let sandbox = Sandbox::new("flw");

    for format in ["json", "csv", "ids"] {
        let output = sandbox.run_in_project(&["logs", "--follow", "--format", format]);
        assert_eq!(output.status.code(), Some(2), "`--format {format}` exits 2");
        assert!(
            output.stdout.is_empty(),
            "and prints nothing at all on stdout: {}",
            String::from_utf8_lossy(&output.stdout),
        );
        let error: serde_json::Value =
            serde_json::from_slice(&output.stderr).expect("errors are JSON on stderr");
        assert_eq!(error["error"], "UsageError");
        assert_eq!(error["details"]["format"], format);
    }
}

#[test]
fn a_closed_pipe_ends_the_command_quietly_rather_than_panicking() {
    // `lazydap watch list --format jsonl | head -1` used to exit 101 with a
    // `Broken pipe` panic across stderr. The reader closing is how `head`
    // works, not a failure.
    let sandbox = Sandbox::new("pipe");
    for expression in ["a", "b", "c"] {
        sandbox.run_in_project(&["watch", "add", expression]);
    }

    // `| true` closes the read end before lazydap writes, which a race with
    // `head` would not guarantee. The exit code travels out on stderr because
    // `$PIPESTATUS` is not POSIX `sh`.
    let script = format!(
        "{{ {} watch list --format jsonl; echo \"rc=$?\" >&2; }} | true",
        LAZYDAP,
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .current_dir(sandbox.project())
        .env("LAZYDAP_INSTANCE", &sandbox.instance)
        .env("LAZYDAP_RUNTIME_DIR", sandbox.root.join("r"))
        .env("LAZYDAP_DATA_DIR", sandbox.root.join("d"))
        .output()
        .expect("run the pipeline");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("rc=0"), "got: {stderr}");
    assert!(!stderr.contains("panicked"), "got: {stderr}");
}

#[test]
fn the_writers_that_bypassed_the_printer_survive_a_closed_pipe_too() {
    // `clap_complete` writes the completion script itself, and clap's usage
    // errors, lazydap's own errors and every stderr warning went out through
    // `eprintln!` — so none of them went through `print_line`, and all of them
    // panicked on `EPIPE`. Piping a completion script into a pager, or folding
    // stderr into a piped stdout to read an error, exited 101.
    //
    // None of these needs a sandbox: `completions` starts nothing, a command
    // clap cannot parse never looks for a daemon, and `break` on a file that
    // is not there is refused before one is started.
    for (what, script, expected) in [
        (
            "the completion script",
            format!("{{ {LAZYDAP} completions bash; echo \"rc=$?\" >&2; }} | true"),
            "rc=0",
        ),
        (
            // Both streams go into the closed pipe, so the exit code has to
            // come back out on a third descriptor to be observable at all.
            "a usage error",
            format!(
                "exec 3>&2; {{ {LAZYDAP} --format json nosuchcommand 2>&1; \
                 echo \"rc=$?\" >&3; }} | true"
            ),
            "rc=2",
        ),
        (
            "a runtime error as JSON",
            format!(
                "exec 3>&2; {{ {LAZYDAP} --format json break /nonexistent.c:1 2>&1; \
                 echo \"rc=$?\" >&3; }} | true"
            ),
            "rc=1",
        ),
        (
            "a runtime error as prose",
            format!(
                "exec 3>&2; {{ {LAZYDAP} --format table break /nonexistent.c:1 2>&1; \
                 echo \"rc=$?\" >&3; }} | true"
            ),
            "rc=1",
        ),
    ] {
        let output = Command::new("sh")
            .arg("-c")
            .arg(&script)
            .output()
            .expect("run the pipeline");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{what} gave: {stderr}");
        assert!(!stderr.contains("panicked"), "{what} gave: {stderr}");
    }
}

#[test]
fn following_a_log_nobody_is_reading_ends_rather_than_waiting_for_a_line() {
    // The first regression this fix caused: the tail print noticed the closed
    // pipe and the answer was thrown away, so `follow_log` went on polling
    // every 200 ms for a line an idle daemon never writes. A hang is worse for
    // a script than the panic it replaced.
    let sandbox = Sandbox::new("flwp");
    sandbox.run_in_project(&["status"]);

    let script = format!("{LAZYDAP} logs --follow --format table | true");
    let output = Command::new("timeout")
        .args(["6", "sh", "-c", &script])
        .current_dir(sandbox.project())
        .env("LAZYDAP_INSTANCE", &sandbox.instance)
        .env("LAZYDAP_RUNTIME_DIR", sandbox.root.join("r"))
        .env("LAZYDAP_DATA_DIR", sandbox.root.join("d"))
        .output()
        .expect("run the pipeline");

    assert_ne!(
        output.status.code(),
        Some(124),
        "`timeout` killed it: --follow waited for a reader that had gone",
    );
    assert_eq!(output.status.code(), Some(0), "and it ended cleanly");
}

#[test]
fn a_format_meant_for_the_debuggee_is_not_read_as_lazydaps_own() {
    // Everything after a bare `--` belongs to the program being debugged.
    // Scanning past it made `-- --format table` turn lazydap's own error
    // reporting to prose for a caller who never asked.
    let sandbox = Sandbox::new("dashes");

    let output = sandbox.run(&["launch", "./x", "--nosuch", "--", "--format", "table"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let error: serde_json::Value =
        serde_json::from_str(&stderr).unwrap_or_else(|error| unreachable!("{error}: {stderr}"));
    assert_eq!(error["error"], "UsageError");
}

#[test]
fn every_usage_error_is_labelled_the_same_way() {
    // `--format ids` on a result that is not a list said `BadRequest` while
    // every other usage mistake said `UsageError`, so a script had to know
    // both.
    let sandbox = Sandbox::new("lbl");

    let output = sandbox.run(&["version", "--format", "ids"]);
    assert_eq!(output.status.code(), Some(2));
    let error: serde_json::Value =
        serde_json::from_slice(&output.stderr).expect("errors are JSON on stderr");
    assert_eq!(error["error"], "UsageError");
    assert!(
        !error["message"]
            .as_str()
            .expect("a message")
            .contains("BadRequest"),
        "the label names the mistake once: {error}",
    );
}

#[test]
fn an_explicit_table_format_is_honoured_even_when_stdout_is_a_pipe() {
    // The guess exists for callers who said nothing. `--format table` is a
    // person saying they want prose, and piping it into `less` or `cat` does
    // not change their mind.
    let sandbox = Sandbox::new("tbl");

    let human = sandbox.run(&["--format", "table", "nosuch"]);
    assert_eq!(human.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&human.stderr);
    assert!(
        serde_json::from_str::<serde_json::Value>(&stderr).is_err(),
        "clap's own text, not JSON: {stderr}",
    );

    // And with nothing said, a pipe still means JSON.
    let machine = sandbox.run(&["nosuch"]);
    let stderr = String::from_utf8_lossy(&machine.stderr);
    let error: serde_json::Value =
        serde_json::from_str(&stderr).unwrap_or_else(|error| unreachable!("{error}: {stderr}"));
    assert_eq!(error["error"], "UsageError");
}

#[test]
fn a_timeout_variable_nothing_can_read_is_reported_rather_than_ignored() {
    // Silently falling back to 30 seconds leaves somebody who exported
    // `LAZYDAP_TIMEOUT=5m` believing every wait in that shell is bounded by
    // five minutes.
    let sandbox = Sandbox::new("tmo");

    for value in ["5m", "  ", "0x10"] {
        let output =
            sandbox.run_in_project_with_env(&[("LAZYDAP_TIMEOUT", value)], &["continue", "--wait"]);
        let expected = if value.trim().is_empty() {
            // Blank is how a shell unsets a variable it cannot remove, so it
            // means "nothing set" and the command gets as far as the daemon.
            Some(1)
        } else {
            Some(2)
        };
        assert_eq!(
            output.status.code(),
            expected,
            "`LAZYDAP_TIMEOUT={value}`: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let sandbox = Sandbox::new("tmo2");
    let output =
        sandbox.run_in_project_with_env(&[("LAZYDAP_TIMEOUT", "5m")], &["continue", "--wait"]);
    assert_eq!(output.status.code(), Some(2), "usage errors exit 2");
    assert!(
        sandbox.daemon_files().is_empty(),
        "and it is refused before a daemon is started: {:?}",
        sandbox.daemon_files(),
    );
}

#[test]
fn doctor_does_not_fail_just_because_one_adapter_is_missing() {
    // `ok` means lazydap can debug something here, not that this machine has
    // every adapter lazydap ships (D093). Exit 1 on a healthy install is
    // the last line of the README, of install.sh and of the Homebrew formula.
    let sandbox = Sandbox::new("doca");
    let bin = sandbox.root.join("bin");
    std::fs::create_dir_all(&bin).expect("create the fake bin directory");
    let codelldb = bin.join("codelldb");
    std::fs::write(&codelldb, "#!/bin/sh\n").expect("write the fake adapter");
    std::fs::set_permissions(&codelldb, std::fs::Permissions::from_mode(0o755))
        .expect("make it executable");

    let path = bin.to_string_lossy().into_owned();
    let output = sandbox
        .run_in_project_with_env(&[("PATH", path.as_str())], &["--format", "json", "doctor"]);

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the report is on stdout");
    let checks = report["checks"].as_array().expect("checks");
    assert!(
        checks
            .iter()
            .any(|check| check["name"] == "adapter.delve" && check["ok"] == false),
        "delve is not on this PATH: {report}",
    );
    assert_eq!(
        report["ok"], true,
        "and codelldb is, which is enough to debug something: {report}",
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn doctor_check_state_reads_the_file_without_a_daemon() {
    // The state file a daemon refuses to start on is exactly the file this
    // check exists for, so it must not need one.
    let sandbox = Sandbox::new("docs");
    sandbox.write_in_project(".lazydap/state.toml", "[[breakpoints]\nfile = \"x.c\"\n");

    let output = sandbox.run_in_project(&["--format", "json", "doctor", "--check-state"]);

    assert_eq!(output.status.code(), Some(1), "a broken file fails");
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the report is still on stdout");
    let state = report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["name"] == "state.file")
        .expect("doctor must say something about the state file");
    assert_eq!(state["ok"], false);
    let detail = state["detail"].as_str().expect("a detail");
    assert!(detail.contains("line 1"), "with the line in it: {detail}");

    assert!(
        sandbox.daemon_files().is_empty(),
        "and no daemon was started to answer it: {:?}",
        sandbox.daemon_files(),
    );
}

#[test]
fn a_broken_launch_json_does_not_hide_the_projects_own_configurations() {
    // `.vscode/launch.json` belongs to VS Code. A stray comma in it must not
    // take away the configurations in `.lazydap/state.toml`, which is where
    // `launches run` finds lazydap's own.
    let sandbox = Sandbox::new("ljson");
    sandbox.write_in_project(
        ".lazydap/state.toml",
        "[[launch_configs]]\nname = \"mine\"\nprogram = \"./app\"\n",
    );
    sandbox.write_in_project(".vscode/launch.json", "{ \"configurations\": [ oops ] }");

    let listed = sandbox.json_in_project(&["--format", "json", "launches", "list"]);

    let names: Vec<&str> = listed["configs"]
        .as_array()
        .expect("configs")
        .iter()
        .map(|config| config["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, vec!["mine"], "got: {listed}");

    let warnings = listed["warnings"].as_array().expect("warnings");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.as_str().is_some_and(|w| w.contains("launch.json"))),
        "and the file that could not be read is named: {listed}",
    );
}
