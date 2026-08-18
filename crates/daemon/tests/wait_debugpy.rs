//! The agent loop against a real debugpy and real Python debuggees.
//!
//! The M18 question is not "does debugpy work" — it is whether a second
//! adapter reaches the *same* answers through the same commands. So these
//! deliberately mirror `wait_codelldb.rs` rather than exploring what debugpy
//! can do: the same `--wait` outcomes, asserted the same way, against
//! `examples/py-fixtures/` instead of `examples/c-fixtures/`. Where an
//! assertion here differs from its C twin, that difference is the finding, and
//! it is commented.
//!
//! Every test skips, loudly, when there is no Python that can import debugpy,
//! so a machine without one still gets a green `cargo test`. Set
//! `LAZYDAP_REQUIRE_ADAPTERS` to turn that skip into a failure — CI does,
//! because a suite that skips itself proves nothing. Empty and `0` count as
//! unset.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const LAZYDAP: &str = env!("CARGO_BIN_EXE_lazydap");

/// One debug session at a time, across the whole file.
///
/// The same discipline `wait_codelldb.rs` explains at length, and for the same
/// reason: `cargo test` runs a file's tests in parallel, the launch handshake
/// has a fifteen-second deadline, and a dozen interpreters starting at once
/// contend for one machine. Python pays less of this than codelldb — there is
/// no LLDB to load and no signature to evaluate — but "less" is not "none",
/// and a suite whose failures depend on how busy the machine is teaches
/// nothing.
///
/// Separate from the codelldb file's mutex, deliberately. A `static` is
/// per-test-binary, so the two suites cannot share one however it is written;
/// what keeps them from overlapping is that each takes turns within itself and
/// each cleans up after itself.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Skip loudly — or fail, when the run was promised an adapter.
///
/// The same discipline `wait_codelldb.rs` explains: a skip and a pass are the
/// same colour in a CI log, so a suite nothing installed an adapter for proved
/// nothing while looking green. CI sets `LAZYDAP_REQUIRE_ADAPTERS` and a
/// missing adapter fails there; a laptop without one sets nothing and still
/// gets a green `cargo test`.
macro_rules! skip_or_fail {
    ($reason:expr) => {{
        let test = std::thread::current()
            .name()
            .unwrap_or("this test")
            .to_string();
        // Empty and `0` count as unset. `FOO= cargo test` is how a shell
        // clears a variable it has already exported, and a run that meant to
        // switch the requirement off should not be failed by it.
        let required = std::env::var("LAZYDAP_REQUIRE_ADAPTERS")
            .is_ok_and(|value| !matches!(value.trim(), "" | "0"));
        assert!(
            !required,
            "{test}: {} — LAZYDAP_REQUIRE_ADAPTERS is set, so this cannot be skipped",
            $reason,
        );
        eprintln!("skipping {test}: {}", $reason);
        return;
    }};
}

/// Claim the machine and check it can run this at all.
macro_rules! require_python {
    () => {{
        // Held for the rest of the test: the guard is returned alongside the
        // interpreter so it lives exactly as long as the test body does.
        let guard = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match find_python() {
            Some(python) => (python, guard),
            None => skip_or_fail!("needs a python3 on PATH that can import debugpy"),
        }
    }};
}

/// The first interpreter on `PATH` that can import debugpy.
///
/// The same rule `crate::adapter::discover_in` applies, on purpose: a test that
/// found its interpreter a different way could pass while the product's own
/// discovery was picking a different one.
fn find_python() -> Option<PathBuf> {
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        for name in ["python3", "python"] {
            let candidate = dir.join(name);
            if !candidate.is_file() {
                continue;
            }
            let usable = Command::new(&candidate)
                .args(["-c", "import debugpy"])
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false);
            if usable {
                return Some(candidate);
            }
        }
    }
    None
}

/// Where a fixture lives. Nothing is built: the source *is* the program.
fn fixture(name: &str) -> PathBuf {
    repo_root().join("examples/py-fixtures").join(name)
}

fn repo_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `crates/daemon`.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root is two levels up")
        .to_path_buf()
}

/// One isolated daemon, its own project directory, cleaned up at the end.
struct Sandbox {
    root: PathBuf,
    project: PathBuf,
    instance: String,
}

impl Sandbox {
    /// Terse names on purpose: a Unix socket path has about a hundred bytes,
    /// and lazydap refuses to bind one that overruns it.
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let instance = format!("{label}{}-{unique}", std::process::id());
        let root = PathBuf::from("/tmp").join(format!("lzdpy-{instance}"));

        for sub in ["r", "d", "p"] {
            std::fs::create_dir_all(root.join(sub)).expect("create the sandbox");
        }
        // `.lazydap/` marks the project root (D024), so the store lands here
        // rather than in whatever repository the tests are run from.
        let project = root.join("p");
        std::fs::create_dir_all(project.join(".lazydap")).expect("create the project marker");

        Self {
            root,
            project,
            instance,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(LAZYDAP)
            .current_dir(&self.project)
            .env("LAZYDAP_INSTANCE", &self.instance)
            .env("LAZYDAP_RUNTIME_DIR", self.root.join("r"))
            .env("LAZYDAP_DATA_DIR", self.root.join("d"))
            .args(args)
            .output()
            .expect("run lazydap")
    }

    fn json(&self, args: &[&str]) -> Value {
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

    /// Launch a fixture. No `--adapter`: which one to use is read off the
    /// `.py`, and a test that passed it explicitly would not be testing that.
    fn launch(&self, program: &Path) -> Value {
        self.json(&[
            "--format",
            "json",
            "launch",
            &program.to_string_lossy(),
            "--stop-on-entry",
        ])
    }

    fn breakpoint(&self, source: &str, line: u32) -> Value {
        let source = fixture(source).display().to_string();
        self.json(&["--format", "json", "break", &format!("{source}:{line}")])
    }

    fn wait(&self, timeout: &str) -> Value {
        self.json(&[
            "--format",
            "json",
            "continue",
            "--wait",
            "--timeout",
            timeout,
        ])
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = self.run(&["disconnect"]);
        let _ = self.run(&["shutdown"]);
        let _ = std::fs::remove_dir_all(&self.root);
        assert_no_orphans();
    }
}

/// Fail if a fixture is still running once a test has finished with it.
///
/// The Python version of the D045 check. It matches on the fixture *directory*
/// rather than a built binary, because these are never compiled: a leaked
/// debuggee is a `python3 .../examples/py-fixtures/spins.py` in the process
/// table. `spins.py` sleeps rather than spinning, so a leak here costs no CPU
/// and would be that much easier to miss.
fn assert_no_orphans() {
    let fixtures = repo_root().join("examples/py-fixtures");
    let output = Command::new("pgrep")
        .args(["-f", &fixtures.display().to_string()])
        .output()
        .expect("run pgrep");

    let survivors: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    // Not an assertion while the test is already failing. This runs in `Drop`,
    // and a second panic during unwind aborts the process — taking the first
    // panic's message, the one that explains the failure, with it. Report the
    // orphans and step aside; the real failure is the one worth reading.
    if std::thread::panicking() {
        if !survivors.is_empty() {
            eprintln!("orphans left by a failing test: {survivors:?}");
        }
        return;
    }
    assert!(
        survivors.is_empty(),
        "a Python debuggee outlived its session — pids {} under {}. \
         The adapter died without stopping it and nothing reaped it; see D045.",
        survivors.join(", "),
        fixtures.display(),
    );
}

/// Children of `parent` whose command line contains `pattern`.
///
/// Matched on the whole command line rather than the process name, because the
/// adapter has no name of its own: it is `python3 -m debugpy.adapter`, and on
/// macOS the executable is called `Python`. Scoped to this daemon's children so
/// a suite running in another worktree keeps its adapters.
fn children_matching(parent: u64, pattern: &str) -> Vec<u32> {
    let output = Command::new("pgrep")
        .args(["-P", &parent.to_string(), "-f", pattern])
        .output()
        .expect("run pgrep");

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// Whether any process is still running one of the Python fixtures.
fn fixtures_running() -> Vec<String> {
    let fixtures = repo_root().join("examples/py-fixtures");
    let output = Command::new("pgrep")
        .args(["-f", &fixtures.display().to_string()])
        .output()
        .expect("run pgrep");

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

fn output_texts(blob: &Value) -> Vec<String> {
    blob["captured_output"]
        .as_array()
        .map(|chunks| {
            chunks
                .iter()
                .filter(|chunk| chunk["category"] == "stdout")
                .filter_map(|chunk| chunk["output"].as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_python_program_is_launched_under_debugpy_without_being_told_to() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("pick");

    let launched = sandbox.launch(&fixture("exits.py"));

    // debugpy reports a stop-on-entry stop as `entry` natively, so D033's
    // renaming has nothing to do — and `raw_reason` stays null, where
    // codelldb's says "exception". That difference *is* the normalisation
    // being visible rather than silent.
    assert_eq!(launched["state"], "paused", "got: {launched}");
    assert_eq!(launched["reason"], "entry", "got: {launched}");
    assert_eq!(
        launched["raw_reason"],
        Value::Null,
        "nothing was renamed, so nothing to disclose: {launched}",
    );

    let status = sandbox.json(&["--format", "json", "status"]);
    assert_eq!(
        status["session"]["adapter"], "debugpy",
        "a `.py` picks debugpy with no --adapter: {status}",
    );
}

#[test]
fn a_python_frame_names_its_source_even_though_debugpy_only_sends_a_path() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("srcn");

    sandbox.launch(&fixture("exits.py"));
    let stack = sandbox.json(&["--format", "json", "stack", "--levels", "1"]);

    // codelldb and delve send `source.name`; debugpy sends only `path`. An
    // agent formatting `frame.source.name` got two languages and a blank, so
    // the file name is filled in from the path lazydap already has (D069).
    let source = &stack["frames"][0]["source"];
    assert_eq!(source["name"], "exits.py", "got: {stack}");
    assert!(
        source["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("exits.py")),
        "got: {stack}",
    );
}

#[test]
fn continuing_to_a_breakpoint_reports_where_and_why_it_stopped() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("bp");

    sandbox.launch(&fixture("exits.py"));
    let added = sandbox.breakpoint("exits.py", 4);
    assert_eq!(added["breakpoints"][0]["verified"], true, "got: {added}");

    let blob = sandbox.wait("20");

    assert_eq!(blob["state"], "paused", "got: {blob}");
    assert_eq!(blob["reason"], "breakpoint", "got: {blob}");
    assert_eq!(blob["frame"]["line"], 4, "got: {blob}");
    // Module-level code, so the frame is `<module>` rather than a function
    // name. codelldb's twin asserts `main` here for the same reason: it is
    // whatever the debuggee's top frame is actually called.
    assert_eq!(blob["frame"]["name"], "<module>", "got: {blob}");

    // The difference from codelldb, and a real one: debugpy's `stopped` event
    // carries no `hitBreakpointIds`, so lazydap has no adapter ids to map back
    // to its own and reports none. The stop still says `breakpoint` and still
    // says where. An agent that branches on *which* breakpoint was hit gets
    // that from codelldb and must fall back to the frame under debugpy — see
    // `docs/reference/debugpy-quirks.md`.
    assert_eq!(
        blob["hit_breakpoint_ids"],
        serde_json::json!([]),
        "debugpy names no ids; if this ever fills in, the quirk is fixed: {blob}",
    );
}

/// Launch without `--stop-on-entry`, then `continue --wait` — the ordinary way
/// to reach a first breakpoint, and the way `launches run` behaves for a
/// configuration that does not set `stopOnEntry`.
///
/// It used to hang here for debugpy and only for debugpy: the program was
/// already running, and debugpy does not answer a `continue` for a thread that
/// is not paused. The acknowledgement timeout that followed was read as a
/// wedged adapter and killed the session. codelldb answers such a `continue`,
/// which is why one adapter's suite could never have found this.
#[test]
fn continuing_a_program_that_is_already_running_reaches_the_breakpoint() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("run");

    sandbox.breakpoint("exits.py", 4);
    let launched = sandbox.json(&[
        "--format",
        "json",
        "launch",
        &fixture("exits.py").to_string_lossy(),
    ]);
    assert_eq!(launched["state"], "running", "got: {launched}");

    let blob = sandbox.wait("20");

    assert_eq!(blob["state"], "paused", "got: {blob}");
    assert_eq!(blob["reason"], "breakpoint", "got: {blob}");
    assert_eq!(blob["frame"]["line"], 4, "got: {blob}");
}

#[test]
fn a_program_that_finishes_reports_its_exit_code() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("exit");

    sandbox.launch(&fixture("exits.py"));
    let blob = sandbox.wait("20");

    assert_eq!(blob["state"], "exited", "got: {blob}");
    assert_eq!(blob["exit_code"], 0, "got: {blob}");
    assert!(
        output_texts(&blob).iter().any(|text| text.contains("x=5")),
        "the debuggee's stdout must survive the run: {blob}",
    );
}

#[test]
fn a_program_that_never_stops_times_out_without_ending_the_session() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("spin");

    sandbox.launch(&fixture("spins.py"));
    let blob = sandbox.wait("2");

    assert_eq!(blob["state"], "timeout", "got: {blob}");

    // The session is still there and still usable — a timeout is this call
    // giving up, not the program being over.
    let status = sandbox.json(&["--format", "json", "status"]);
    assert_eq!(status["session"]["state"], "running", "got: {status}");
}

#[test]
fn an_uncaught_exception_ends_the_program_without_pausing_it() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("crash");

    sandbox.launch(&fixture("crashes.py"));
    let blob = sandbox.wait("20");

    // Not the C twin's outcome, and the difference is the point. A segfault is
    // a signal the debugger sees whether or not anybody asked; an uncaught
    // Python exception is only a stop if the client requested one with
    // `setExceptionBreakpoints`, and lazydap sends no exception filters. So
    // the program dies the way it would have died unattended: exit code 1,
    // traceback on stderr, no pause to inspect.
    //
    // That is worth knowing rather than working around here: turning it into a
    // pause means choosing exception filters for everybody, which is a
    // decision M18 does not get to make on its own.
    assert_eq!(blob["state"], "exited", "got: {blob}");
    assert_eq!(blob["exit_code"], 1, "got: {blob}");

    let stderr: String = blob["captured_output"]
        .as_array()
        .map(|chunks| {
            chunks
                .iter()
                .filter(|chunk| chunk["category"] == "stderr")
                .filter_map(|chunk| chunk["output"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        stderr.contains("ValueError: nothing here"),
        "the traceback is the only account of the failure, so it must survive: {blob}",
    );
}

#[test]
fn a_lot_of_output_arrives_in_order() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("chat");

    sandbox.launch(&fixture("chatty.py"));
    sandbox.breakpoint("chatty.py", 7);
    let blob = sandbox.wait("20");

    assert_eq!(blob["state"], "paused", "got: {blob}");

    // Ordering, not volume: `print` reaches us as its own output events, and
    // whatever they are chunked into, "line 0" must precede "line 199".
    let joined = output_texts(&blob).join("");
    let first = joined.find("line 0").expect("the first line");
    let last = joined.find("line 199").expect("the last line");
    assert!(first < last, "output arrived out of order: {joined}");
}

/// D045 for Python: the adapter is killed mid-wait, and nothing is left behind.
///
/// What this can and cannot show, stated plainly, because the difference
/// matters to whoever reads a failure here. It shows the *outcome*: the wait
/// ends as `adapter_died` rather than sitting until its timeout (D022), and no
/// debuggee survives. It does **not** discriminate lazydap's reap from
/// debugpy's own cleanup — observed on 1.8.21, the launcher kills the debuggee
/// itself the moment the adapter socket drops, so the pid is already gone when
/// the reap looks at it.
///
/// The reap is therefore belt-and-braces here rather than the only thing
/// standing between a user and an orphan. That it works for an
/// interpreter-run program at all is checked where it can be:
/// `debuggee::tests::a_script_running_under_an_interpreter_is_recognised_and_killed`.
#[test]
fn a_killed_adapter_takes_its_python_debuggee_with_it() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("dead");

    sandbox.launch(&fixture("spins.py"));
    let daemon_pid = sandbox.json(&["--format", "json", "status"])["daemon_pid"]
        .as_u64()
        .expect("a daemon pid");

    // Kill the adapter while a wait is in flight — nothing sends `terminated`,
    // which is the case D022 exists for, and nothing stops the debuggee, which
    // is the case D045 exists for.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(700));
        for pid in children_matching(daemon_pid, "debugpy.adapter") {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).output();
        }
    });

    let blob = sandbox.wait("20");
    assert_eq!(blob["state"], "adapter_died", "got: {blob}");
    assert!(
        blob["elapsed_ms"].as_u64().unwrap_or(u64::MAX) < 20_000,
        "it must not have waited out the timeout: {blob}",
    );

    // `spins.py` sleeps rather than spinning, so an orphan costs no CPU and
    // would be that much easier to miss. Give the cleanup a moment to land.
    for _ in 0..20 {
        if fixtures_running().is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    unreachable!(
        "the debuggee outlived its adapter — pids {}",
        fixtures_running().join(", "),
    );
}

#[test]
fn variables_and_expressions_read_the_paused_frame() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("eval");

    sandbox.launch(&fixture("exits.py"));
    sandbox.breakpoint("exits.py", 4);
    sandbox.wait("20");

    let scopes = sandbox.json(&["--format", "json", "scopes"]);
    let names: Vec<&str> = scopes["scopes"]
        .as_array()
        .map(|scopes| {
            scopes
                .iter()
                .filter_map(|scope| scope["name"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        names.contains(&"Locals"),
        "debugpy names the frame's own scope `Locals`: {scopes}",
    );

    let evaluated = sandbox.json(&["--format", "json", "eval", "x * 3"]);
    assert_eq!(evaluated["value"], "15", "got: {evaluated}");
    assert_eq!(
        evaluated["type_name"], "int",
        "debugpy names the type, and lazydap passes it through: {evaluated}",
    );

    // An expression Python cannot evaluate is the adapter's refusal, reported
    // as one rather than as an empty success.
    let refused = sandbox.run(&["--format", "json", "eval", "nope"]);
    assert!(
        !refused.status.success(),
        "an undefined name must not look like a value: {}",
        String::from_utf8_lossy(&refused.stdout),
    );
}

#[test]
fn an_adapter_whose_session_ended_is_reaped_rather_than_left_waiting_for_a_disconnect() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("reap");

    sandbox.launch(&fixture("exits.py"));
    let daemon_pid = sandbox.json(&["--format", "json", "status"])["daemon_pid"]
        .as_u64()
        .expect("a daemon pid");
    assert_eq!(sandbox.wait("20")["state"], "exited");

    // The C twin, and the same finding: debugpy also keeps its socket open
    // after `terminated` and waits to be disconnected from, so a daemon that
    // only ever read from it accumulated one adapter per session (D094).
    // Nothing here says `disconnect` — the daemon does it for itself.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let survivors = loop {
        let found = children_matching(daemon_pid, "debugpy.adapter");
        if found.is_empty() || std::time::Instant::now() >= deadline {
            break found;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    assert!(
        survivors.is_empty(),
        "the adapter outlived the session it was serving: {survivors:?}",
    );
}

#[test]
fn disconnecting_without_terminating_says_so_honestly_when_debugpy_cannot_detach() {
    let (_python, _turn) = require_python!();
    let sandbox = Sandbox::new("keep");

    sandbox.launch(&fixture("spins.py"));
    assert_eq!(sandbox.wait("1")["state"], "timeout");

    // debugpy does not advertise DAP's `supportTerminateDebuggee` — it sends a
    // *differently spelled* `supportsTerminateDebuggee` and then never answers
    // a `disconnect` carrying `terminateDebuggee: false` at all. lazydap used
    // to send one anyway, wait out the ten-second request timeout, kill the
    // adapter, watch the debuggee die with it, and report
    // `terminated_debuggee: false`. Both halves of that were wrong (D095).
    let started = std::time::Instant::now();
    let answer = sandbox.json(&["--format", "json", "disconnect", "--no-terminate"]);

    assert_eq!(
        answer["terminated_debuggee"], true,
        "the program does die, and the answer has to say so: {answer}",
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "it must not wait out a request debugpy never answers: {:?}",
        started.elapsed(),
    );
    // `assert_no_orphans` in `Drop` is the other half: it really is gone.
}
