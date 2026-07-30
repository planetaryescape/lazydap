//! `--wait`, against a real codelldb and real debuggees.
//!
//! The list of cases is `docs/blueprint/10-async-to-sync.md` §"Tests required
//! for `--wait`". They are here rather than in unit tests because the thing
//! being checked is what an *adapter* does: a fake that produced these
//! outcomes would only be checking that the fake matches the assertions
//! (non-negotiable #7). The event arithmetic — watermarks, coalescing,
//! output caps — is unit-tested separately and deterministically in
//! `crates/daemon/src/wait.rs`.
//!
//! Every test skips, loudly, when codelldb or a C compiler is missing, so a
//! machine without them still gets a green `cargo test` rather than a wall of
//! unrelated failures.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const LAZYDAP: &str = env!("CARGO_BIN_EXE_lazydap");

/// One debug session at a time, across the whole file.
///
/// Every test here spawns a real codelldb, which loads LLDB, maps a debuggee
/// and talks over TCP. `cargo test` runs the file's tests in parallel, so a
/// dozen of those start at once and contend for the same machine — and the
/// launch handshake has a 15-second deadline. Under load that deadline is
/// reached before the adapter is ready, and the suite fails in a way that has
/// nothing to do with lazydap: 12 of 13 timed out on a reviewer's machine
/// while passing in isolation.
///
/// So they take turns. It costs a few seconds of wall clock and buys a suite
/// whose failures mean something.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Claim the machine and check it can run this at all.
///
/// Skips, loudly, when codelldb or a C compiler is missing. The thread name is
/// the test's own name under the default harness, which is what makes the skip
/// line say which test went quiet.
macro_rules! require_toolchain {
    () => {{
        // Held for the rest of the test: the guard is returned alongside the
        // toolchain so it lives exactly as long as the test body does.
        let guard = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match Toolchain::find() {
            Some(toolchain) => (toolchain, guard),
            None => {
                eprintln!(
                    "skipping {}: needs codelldb on PATH and a C compiler",
                    std::thread::current().name().unwrap_or("this test"),
                );
                return;
            }
        }
    }};
}

struct Toolchain {
    compiler: PathBuf,
}

impl Toolchain {
    fn find() -> Option<Self> {
        which("codelldb")?;
        let compiler = which("cc")
            .or_else(|| which("gcc"))
            .or_else(|| which("clang"))?;
        Some(Self { compiler })
    }

    /// Build one of `examples/c-fixtures/`, with debug info and no optimising
    /// — a breakpoint on a line that the optimiser folded away is not a test
    /// of anything.
    ///
    /// Built to a **fixed path**, and skipped when it is already newer than
    /// its source. Not merely to save a compile: macOS evaluates a binary's
    /// signature the first time a given *inode* is executed, and a debugger
    /// attaching to a brand-new one pays that cost in full. Compiling to a
    /// fresh temporary path per test made every launch take about thirteen
    /// seconds against a fifteen-second handshake deadline — passing alone,
    /// failing the moment the machine was busy. A stable path is evaluated
    /// once and warm thereafter, which took the same launch to under half a
    /// second. See `docs/reference/codelldb-quirks.md` quirk 5 for the
    /// pathological version of the same mechanism.
    fn build(&self, fixture: &str) -> PathBuf {
        let source = repo_root().join("examples/c-fixtures").join(fixture);
        let out_dir = repo_root().join("target/debug/c-fixtures");
        std::fs::create_dir_all(&out_dir).expect("create the fixture directory");
        let binary = out_dir.join(fixture.trim_end_matches(".c"));

        if is_fresh(&binary, &source) {
            return binary;
        }

        let output = Command::new(&self.compiler)
            .args(["-g", "-O0", "-pthread"])
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .expect("run the C compiler");
        assert!(
            output.status.success(),
            "could not build {fixture}: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        binary
    }
}

/// Whether `binary` exists and is at least as new as `source`.
fn is_fresh(binary: &Path, source: &Path) -> bool {
    let modified = |path: &Path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
    };
    match (modified(binary), modified(source)) {
        (Some(built), Some(written)) => built >= written,
        _ => false,
    }
}

fn which(binary: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
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
        let root = PathBuf::from("/tmp").join(format!("lzd-{instance}"));

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
        let source = repo_root()
            .join("examples/c-fixtures")
            .join(source)
            .display()
            .to_string();
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
/// A debuggee outlives its test when the adapter dies without stopping it,
/// which is the bug D045 exists for. It is invisible without this: the suite
/// goes green, the process is reparented to init, and it busy-loops until
/// somebody notices. Forty-six of them had accumulated across worktrees before
/// anybody did — so a leak now fails the test that caused it, loudly, rather
/// than being left for a future count.
///
/// Scoped to *this* build's fixture directory. Several worktrees run this suite
/// at once and a blanket match on the fixture names would make each of them
/// fail on the others' processes.
fn assert_no_orphans() {
    let fixtures = repo_root().join("target/debug/c-fixtures");
    let output = Command::new("pgrep")
        .args(["-f", &fixtures.display().to_string()])
        .output()
        .expect("run pgrep");

    let survivors: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    assert!(
        survivors.is_empty(),
        "a debuggee outlived its session — pids {} under {}. \
         The adapter died without stopping it and nothing reaped it; see D045.",
        survivors.join(", "),
        fixtures.display(),
    );
}

/// Processes named `name` whose parent is `parent`.
///
/// Used to reach one test's adapter without touching anybody else's.
fn children_of(parent: u64, name: &str) -> Vec<u32> {
    let output = Command::new("pgrep")
        .args(["-P", &parent.to_string(), "-x", name])
        .output()
        .expect("run pgrep");

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
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
fn continuing_to_a_breakpoint_reports_where_and_why_it_stopped() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("bp");
    let program = toolchain.build("exits.c");

    sandbox.launch(&program);
    let added = sandbox.breakpoint("exits.c", 6);
    assert_eq!(added["breakpoints"][0]["verified"], true, "got: {added}");

    let blob = sandbox.wait("20");

    assert_eq!(blob["state"], "paused", "got: {blob}");
    assert_eq!(blob["reason"], "breakpoint", "got: {blob}");
    assert_eq!(blob["frame"]["line"], 6, "got: {blob}");
    assert_eq!(blob["frame"]["name"], "main", "got: {blob}");
    assert!(
        blob["hit_breakpoint_ids"]
            .as_array()
            .is_some_and(|ids| !ids.is_empty()),
        "the stop must name the breakpoint that caused it: {blob}",
    );

    // Breakpoint updates have to carry *our* id, not just the adapter's.
    // codelldb verifies lazily, so these arrive mid-run; an update a caller
    // cannot match against the ids `break --list` gave them names nothing.
    let ours = added["breakpoints"][0]["id"].clone();
    for update in blob["breakpoint_updates"].as_array().unwrap_or(&Vec::new()) {
        assert_eq!(
            update["id"], ours,
            "a breakpoint update must be correlatable with break --list: {blob}",
        );
    }
}

#[test]
fn continuing_to_the_end_reports_the_exit_code_and_the_last_of_the_output() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("exit");
    let program = toolchain.build("exits.c");

    sandbox.launch(&program);
    let blob = sandbox.wait("20");

    assert_eq!(blob["state"], "exited", "got: {blob}");
    assert_eq!(blob["exit_code"], 0, "got: {blob}");
    assert!(blob["frame"].is_null(), "an exited program has no frame");
    assert!(
        output_texts(&blob)
            .iter()
            .any(|text| text.contains("about to finish")),
        "the output the program produced on its way out belongs in the blob: {blob}",
    );
}

#[test]
fn a_program_that_segfaults_ends_the_wait_rather_than_hanging_it() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("crash");
    let program = toolchain.build("crashes.c");

    sandbox.launch(&program);
    let blob = sandbox.wait("20");

    // Which of these an adapter chooses is its business — LLDB pauses on the
    // signal, others tear the session down. What must not happen is a wait
    // that sits there until its timeout.
    let state = blob["state"].as_str().unwrap_or_default();
    assert!(
        matches!(state, "paused" | "terminated" | "exited"),
        "a crash has to reach a stable state: {blob}",
    );
    if state == "paused" {
        assert_eq!(
            blob["reason"], "exception",
            "a segfault is an exception-class stop: {blob}",
        );
    }
}

#[test]
fn a_program_that_never_stops_times_out_and_keeps_running() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("spin");
    let program = toolchain.build("spins.c");

    sandbox.launch(&program);
    let blob = sandbox.wait("2");

    assert_eq!(blob["state"], "timeout", "got: {blob}");
    assert!(
        blob["elapsed_ms"].as_u64().unwrap_or(0) >= 2_000,
        "the wait should have lasted about as long as it was given: {blob}",
    );

    // The program was not paused behind the caller's back (D3): the session
    // is still running, and a `pause` is how you would stop it.
    let status = sandbox.json(&["--format", "json", "status"]);
    assert_eq!(status["session"]["state"], "running", "got: {status}");
}

#[test]
fn waiting_with_no_timeout_at_all_blocks_rather_than_falling_over() {
    // `--timeout 0` is documented as "wait forever". Expressing that as a very
    // large `Duration` panicked the client: `Instant + Duration` overflows.
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("forever");
    let program = toolchain.build("exits.c");

    sandbox.launch(&program);
    let blob = sandbox.json(&["--format", "json", "continue", "--wait", "--timeout", "0"]);

    assert_eq!(blob["state"], "exited", "got: {blob}");
    assert_eq!(blob["exit_code"], 0, "got: {blob}");
}

#[test]
fn a_paused_program_can_be_interrupted_after_a_timeout() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("pause");
    let program = toolchain.build("spins.c");

    sandbox.launch(&program);
    assert_eq!(sandbox.wait("1")["state"], "timeout");

    let blob = sandbox.json(&["--format", "json", "pause", "--wait", "--timeout", "20"]);
    assert_eq!(blob["state"], "paused", "got: {blob}");
    assert!(blob["frame"].is_object(), "a pause has a frame too: {blob}");
}

#[test]
fn a_chatty_program_s_output_arrives_whole_and_in_order() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("chat");
    let program = toolchain.build("chatty.c");

    sandbox.launch(&program);
    sandbox.breakpoint("chatty.c", 10);
    let blob = sandbox.wait("20");

    assert_eq!(blob["state"], "paused", "got: {blob}");

    let printed = output_texts(&blob).join("");
    assert!(printed.contains("line 0"), "got: {printed:?}");
    assert!(printed.contains("line 199"), "got: {printed:?}");

    let first = printed.find("line 0").expect("line 0");
    let last = printed.find("line 199").expect("line 199");
    assert!(
        first < last,
        "output has to arrive in the order it was printed: {printed:?}",
    );
}

#[test]
fn a_wait_ends_when_the_adapter_is_killed_out_from_under_it() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("dead");
    let program = toolchain.build("spins.c");

    sandbox.launch(&program);
    let daemon_pid = sandbox.json(&["--format", "json", "status"])["daemon_pid"]
        .as_u64()
        .expect("a daemon pid");

    // Kill the adapter while a wait is in flight. Nothing sends `terminated`,
    // which is exactly the case D022 exists for: without the synthetic ending
    // the wait sits there until its timeout and then lies about why.
    //
    // *This* daemon's adapter, found by parentage. A blanket `pkill codelldb`
    // takes out the adapters belonging to every other test running in
    // parallel, which is a fine way to spend an afternoon debugging.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(700));
        for pid in children_of(daemon_pid, "codelldb") {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).output();
        }
    });

    let blob = sandbox.wait("20");
    assert_eq!(blob["state"], "adapter_died", "got: {blob}");
    assert!(
        blob["elapsed_ms"].as_u64().unwrap_or(u64::MAX) < 20_000,
        "it must not have waited out the timeout: {blob}",
    );
}

#[test]
fn a_multi_threaded_stop_is_coherent_about_which_threads_stopped() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("thr");
    let program = toolchain.build("threads.c");

    sandbox.launch(&program);
    sandbox.breakpoint("threads.c", 15);
    let blob = sandbox.wait("20");

    assert_eq!(blob["state"], "paused", "got: {blob}");
    assert!(blob["thread_id"].is_number(), "got: {blob}");

    // How many threads the adapter reports as separately stopped is a race by
    // construction — that is what the coalescing window is *for* — so this
    // asserts the invariant instead: whatever ended up in the list, the thread
    // that named the blob is not also in it, and the program really is
    // multi-threaded.
    let named = blob["thread_id"].as_i64().expect("a thread id");
    let also: Vec<i64> = blob["additional_stopped_threads"]
        .as_array()
        .map(|ids| ids.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();
    assert!(
        !also.contains(&named),
        "the first stop is not an extra: {blob}"
    );

    let threads = sandbox.json(&["--format", "json", "threads"]);
    let count = threads["threads"].as_array().map(Vec::len).unwrap_or(0);
    assert!(count > 1, "the fixture is multi-threaded: {threads}");
}

#[test]
fn two_waits_sent_at_once_both_come_back_with_an_answer() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("queue");
    let program = toolchain.build("exits.c");

    sandbox.launch(&program);
    sandbox.breakpoint("exits.c", 6);

    // D021: execution requests queue rather than pipelining. Neither of these
    // may hang, and neither may come back with a half-built blob.
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| sandbox.wait("30"));
        let second = scope.spawn(|| sandbox.wait("30"));
        (first.join().expect("first"), second.join().expect("second"))
    });

    for blob in [&first, &second] {
        let state = blob["state"].as_str().unwrap_or_default();
        assert!(
            matches!(state, "paused" | "exited" | "terminated"),
            "a queued wait still has to reach a stable state: {blob}",
        );
        assert!(blob["elapsed_ms"].is_number(), "got: {blob}");
    }

    // The queue means one *run* each, not one message each. The permit is held
    // for the whole wait, so the second continue cannot start until the first
    // has reported its stop — and therefore cannot hand back the first's stop
    // as its own. Two waits, two different outcomes.
    let states: Vec<&str> = [&first, &second]
        .iter()
        .map(|blob| blob["state"].as_str().unwrap_or_default())
        .collect();
    assert!(
        states.contains(&"paused")
            && (states.contains(&"exited") || states.contains(&"terminated")),
        "one wait should have taken the breakpoint and the other the ending, got: {states:?}",
    );
}

#[test]
fn a_breakpoint_set_in_one_session_applies_to_the_next_one() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("persist");
    let program = toolchain.build("exits.c");

    // First session: set the breakpoint, then throw the session away.
    sandbox.launch(&program);
    sandbox.breakpoint("exits.c", 6);
    sandbox.json(&["--format", "json", "disconnect"]);

    // A second launch has to apply it during its own configuration phase —
    // nobody sets it again.
    let launched = sandbox.launch(&program);
    assert_eq!(
        launched["breakpoints"].as_array().map(Vec::len),
        Some(1),
        "the launch should report the breakpoint it applied: {launched}",
    );
    assert_eq!(
        launched["breakpoints"][0]["verified"], true,
        "and the adapter should have taken it: {launched}",
    );

    let blob = sandbox.wait("20");
    assert_eq!(blob["state"], "paused", "got: {blob}");
    assert_eq!(blob["frame"]["line"], 6, "got: {blob}");
}

#[test]
fn breakpoints_survive_the_daemon_that_recorded_them() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("reboot");
    let program = toolchain.build("exits.c");

    let added = sandbox.breakpoint("exits.c", 6);
    let id = added["breakpoints"][0]["id"].clone();
    assert!(
        !added["applied_to_session"].as_bool().unwrap_or(true),
        "there is no session yet, and it must not claim otherwise: {added}",
    );

    // The daemon writes on the way out; a new one reads what it wrote.
    sandbox.json(&["--format", "json", "shutdown"]);

    let listed = sandbox.json(&["--format", "json", "break", "--list"]);
    assert_eq!(
        listed["breakpoints"][0]["id"], id,
        "the id has to be the same one, or a script's `--remove --id` is a lottery: {listed}",
    );

    sandbox.launch(&program);
    assert_eq!(sandbox.wait("20")["frame"]["line"], 6);
}

#[test]
fn listing_ids_feeds_removing_by_id() {
    // The composability criterion from the milestone:
    // `break --list --format ids | xargs -I{} lazydap break --remove --id {}`
    let (_toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("xargs");

    for line in [6, 7] {
        sandbox.breakpoint("exits.c", line);
    }

    let listed = sandbox.run(&["--format", "ids", "break", "--list"]);
    let ids: Vec<String> = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(ids.len(), 2, "got: {ids:?}");

    for id in &ids {
        let removed = sandbox.json(&["--format", "json", "break", "--remove", "--id", id]);
        assert_eq!(removed["action"], "removed", "got: {removed}");
        assert!(
            removed["not_found"].as_array().is_some_and(Vec::is_empty),
            "an id straight out of --format ids must not be stale: {removed}",
        );
    }

    let left = sandbox.json(&["--format", "json", "break", "--list"]);
    assert_eq!(left["breakpoints"].as_array().map(Vec::len), Some(0));
}
