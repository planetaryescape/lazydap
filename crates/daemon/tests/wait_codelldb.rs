//! `--wait`, against a real codelldb and real debuggees.
//!
//! The list of cases is `docs/blueprint/10-async-to-sync.md` §"Tests required
//! for `--wait`", plus what the daemon reports *at* the stop a wait reached —
//! a pause's reason, a thread's name, an expression that could not be
//! evaluated, a window over a large variable. They are here rather than in
//! unit tests because the thing being checked is what an *adapter* does: a
//! fake that produced these outcomes would only be checking that the fake
//! matches the assertions (non-negotiable #7). The event arithmetic —
//! watermarks, coalescing, output caps — is unit-tested separately and
//! deterministically in `crates/daemon/src/wait.rs`.
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

    // codelldb implements `pause` with the same `SIGSTOP` it uses for
    // stop-on-entry, and LLDB calls a signal stop an exception — so a pause the
    // caller asked for reported `reason: "exception"`, which reads as a crash.
    // D064: it is named for what was asked, with the adapter's word kept.
    assert_eq!(blob["reason"], "pause", "got: {blob}");
    assert_eq!(blob["raw_reason"], "exception", "nothing is hidden: {blob}");
}

#[test]
fn a_running_program_s_threads_are_reported_without_inventing_a_name() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("tnm");
    let program = toolchain.build("spins.c");

    sandbox.launch(&program);
    // Not `--wait`: the point is to ask while the program is running.
    sandbox.json(&["--format", "json", "continue"]);
    std::thread::sleep(std::time::Duration::from_millis(500));

    // codelldb answers a running program's `threads` with one nameless thread
    // 0 — a placeholder. lazydap used to fill that in as "thread 0", which
    // reads like an answer (D064).
    let answer = sandbox.json(&["--format", "json", "threads"]);
    let threads = answer["threads"].as_array().expect("an array");
    assert!(
        threads
            .iter()
            .all(|thread| thread["name"] != "thread 0" && thread["name"] != "thread 1"),
        "a name lazydap made up is worse than no name: {answer}",
    );
}

#[test]
fn output_kept_past_the_cap_is_a_prefix_rather_than_a_splice() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("flood");
    let program = toolchain.build("floods.c");

    sandbox.launch(&program);
    sandbox.breakpoint("floods.c", 26);
    let blob = sandbox.wait("60");

    assert_eq!(blob["state"], "paused", "got: {}", blob["state"]);
    assert_eq!(blob["output_truncated"], true, "the cap must have been hit");

    // codelldb hands the debuggee a pty, so its newlines arrive as CRLF.
    let kept = output_texts(&blob).join("").replace("\r\n", "\n");
    assert!(
        !kept.contains("MARKER-AFTER-THE-CAP"),
        "text printed after the cap was reached was spliced onto the cut",
    );

    let printed = "x".repeat(1000) + "\n";
    let printed = printed.repeat(1500) + "MARKER-AFTER-THE-CAP\n";
    assert!(
        printed.starts_with(&kept),
        "`output_truncated` means the tail was cut, so what is kept has to be \
         a prefix of what ran — kept {} of {} bytes",
        kept.len(),
        printed.len(),
    );
}

#[test]
fn an_expression_codelldb_could_not_evaluate_fails_rather_than_answering() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("evl");
    let program = toolchain.build("inspects.c");

    sandbox.launch(&program);
    sandbox.breakpoint("inspects.c", 20);
    assert_eq!(sandbox.wait("30")["state"], "paused");

    // codelldb answers this with a *successful* `evaluate` whose result is
    // `<error: invalid value object>`, so it used to reach callers as a value
    // with no error and exit 0 (D068).
    let failed = sandbox.run(&["--format", "json", "eval", "*(int *)0"]);
    assert!(
        !failed.status.success(),
        "an error the adapter hid in the value is still an error: {}",
        String::from_utf8_lossy(&failed.stdout),
    );

    // And an expression that works still works — the predicate must not cost
    // anybody a real answer.
    let value = sandbox.json(&["--format", "json", "eval", "sum"]);
    assert_eq!(value["value"], "1999", "got: {value}");

    // The known limit, asserted so it stays visible. codelldb writes
    // `<read memory from 0x4 failed (0 of 4 bytes read)>` here, which *is* an
    // error — but " failed" inside angle brackets is also a summary string a
    // real program can have, and failing those would cost a caller values they
    // could otherwise read. The narrow prefix is the defensible line (D074).
    let known_limit = sandbox.json(&["--format", "json", "eval", "*nowhere"]);
    assert!(
        known_limit["value"]
            .as_str()
            .is_some_and(|value| value.contains("failed")),
        "got: {known_limit}",
    );
}

#[test]
fn variables_honours_its_window_even_though_codelldb_ignores_it() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("vars");
    let program = toolchain.build("inspects.c");

    sandbox.launch(&program);
    sandbox.breakpoint("inspects.c", 20);
    assert_eq!(sandbox.wait("30")["state"], "paused");

    let scopes = sandbox.json(&["--format", "json", "scopes"]);
    let locals = scopes["scopes"][0]["variables_reference"].to_string();
    let answer = sandbox.json(&["--format", "json", "variables", "--reference", &locals]);

    let big = answer["variables"]
        .as_array()
        .expect("an array")
        .iter()
        .find(|variable| variable["name"] == "big")
        .expect("the big array is a local");

    // codelldb does not declare `supportsVariablePaging` and ignores all three
    // arguments, so this used to answer with all 2000 entries from `[0]`
    // (D067).
    let window = sandbox.json(&[
        "--format",
        "json",
        "variables",
        "--reference",
        &big["variables_reference"].to_string(),
        "--start",
        "100",
        "--count",
        "5",
    ]);
    let window = window["variables"].as_array().expect("an array");
    assert_eq!(window.len(), 5, "got: {window:?}");
    assert_eq!(window[0]["name"], "[100]", "got: {window:?}");
    assert_eq!(window[4]["name"], "[104]", "got: {window:?}");

    // And the adapter's own answer to "what expression names this row", which
    // is the only thing that turns `[100]` into something `eval` accepts
    // (D069).
    assert_eq!(window[0]["evaluate_name"], "big[100]", "got: {window:?}");
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

/// The error a refused command printed to stderr.
fn refusal(output: &Output, command: &[&str]) -> Value {
    assert!(
        !output.status.success(),
        "`lazydap {}` should have failed",
        command.join(" "),
    );
    serde_json::from_slice(&output.stderr).unwrap_or_else(|error| {
        unreachable!(
            "`lazydap {}` printed something that is not JSON ({error}): {}",
            command.join(" "),
            String::from_utf8_lossy(&output.stderr),
        )
    })
}

#[test]
fn a_crash_inside_a_library_names_the_frame_in_the_user_s_code() {
    // The hunter's segfault: codelldb stopped in
    // `_platform_strcmp$VARIANT$Base`, which has a `source_reference` and no
    // path, and finding `lookup_key` took a second `stack` call. `frame` still
    // says where it stopped — overwriting it would be lying about that — and
    // `user_frame` answers the question the caller actually had (D078).
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("usrfrm");

    let program = toolchain.build("library_crash.c");
    sandbox.json(&["--format", "json", "launch", &program.to_string_lossy()]);
    let blob = sandbox.wait("30");

    assert_eq!(blob["state"], "paused", "got: {blob}");
    let top = &blob["frame"];
    assert!(
        top["source"]["path"].is_null(),
        "the premise of this test is a top frame with no path: {top}",
    );

    let user = &blob["user_frame"];
    assert_eq!(user["name"], "lookup_key", "got: {user}");
    assert!(
        user["source"]["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("library_crash.c")),
        "the user frame has to name a file somebody can open: {user}",
    );
    assert_ne!(
        user["id"], top["id"],
        "and be a different frame from the one it stopped in",
    );

    // And the locals that explain it, which are the *responsible* frame's:
    // `strcmp` has none worth reading, `lookup_key` has the key it was given.
    let locals = &blob["locals"];
    assert_eq!(locals["frame_id"], user["id"], "got: {locals}");
    let names: Vec<&str> = locals["variables"]
        .as_array()
        .expect("locals")
        .iter()
        .filter_map(|variable| variable["name"].as_str())
        .collect();
    assert!(names.contains(&"wanted"), "got: {names:?}");
}

#[test]
fn a_handle_from_an_earlier_stop_is_refused_rather_than_answered() {
    // The dangerous half of the finding. A `variables_reference` the caller
    // remembered across a `continue` used to reach the adapter, which had since
    // recycled that number for another frame — so somebody else's variables
    // came back under exit 0, with nothing saying the question was about a
    // moment that had passed (D075).
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("stale");

    let program = toolchain.build("inspects.c");
    // Inside the loop, so it is hit again and again and there is a genuine
    // second stop for the first stop's handles to go stale against.
    sandbox.breakpoint("inspects.c", 14);
    sandbox.launch(&program);

    let first = sandbox.wait("30");
    assert_eq!(first["state"], "paused", "got: {first}");
    let stale_reference = first["locals"]["variables_reference"].to_string();
    let stale_frame = first["frame"]["id"].to_string();

    // Good at the stop it belongs to.
    let here = sandbox.json(&[
        "--format",
        "json",
        "variables",
        "--reference",
        &stale_reference,
    ]);
    assert!(
        here["variables"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "the handle has to work where it was issued: {here}",
    );

    let second = sandbox.wait("30");
    assert_eq!(second["state"], "paused", "got: {second}");
    assert_ne!(
        second["locals"]["variables_reference"], first["locals"]["variables_reference"],
        "a new stop must not reissue the previous stop's handle",
    );

    for command in [
        vec![
            "--format",
            "json",
            "variables",
            "--reference",
            &stale_reference,
        ],
        vec!["--format", "json", "eval", "i", "--frame", &stale_frame],
    ] {
        let error = refusal(&sandbox.run(&command), &command);
        assert_eq!(error["error"], "StaleHandle", "got: {error}");
        assert_eq!(error["details"]["stale"], true, "got: {error}");
    }
}

#[test]
fn a_frame_id_nobody_handed_out_says_where_frame_ids_come_from() {
    // `--frame 0` is the obvious thing to type and is always wrong: the flag
    // takes an opaque id, not a position in the stack. It used to reach
    // codelldb, which answered `can't evaluate expressions when the process is
    // running` about a program that was plainly stopped — and an agent reading
    // that starts polling something that is never going to move (D075).
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("badfrm");

    let program = toolchain.build("inspects.c");
    sandbox.launch(&program);

    let status = sandbox.json(&["--format", "json", "status"]);
    assert_eq!(status["session"]["state"], "paused", "got: {status}");

    for command in [
        vec!["--format", "json", "eval", "sum", "--frame", "0"],
        vec!["--format", "json", "scopes", "--frame", "0"],
    ] {
        let error = refusal(&sandbox.run(&command), &command);
        assert_eq!(error["error"], "BadRequest", "got: {error}");

        let message = error["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("lazydap stack"),
            "a refusal has to say where a good one comes from: {error}",
        );
        assert!(
            !message.contains("running"),
            "and must never claim a stopped program is running: {error}",
        );
    }
}

#[test]
fn what_the_daemon_reports_at_a_stop_is_bounded_and_does_not_contradict_itself() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("atstop");

    let program = toolchain.build("inspects.c");
    sandbox.breakpoint("inspects.c", 20);
    let launched = sandbox.launch(&program);

    // codelldb answers the launch with `verified: true` *and* `Resolved
    // locations: 0`, then corrects itself by event a moment later. Held
    // together in one response those two made a working breakpoint look
    // broken (D077).
    for breakpoint in launched["breakpoints"].as_array().expect("breakpoints") {
        if breakpoint["verified"] == true {
            assert!(
                breakpoint["message"].is_null(),
                "a verified breakpoint has nothing to explain: {breakpoint}",
            );
        }
    }

    // The program is stopped at entry, so there is nothing to interrupt. This
    // used to hand back the stop it was already sitting at, wearing a fresh
    // `elapsed_ms` (D081).
    let command = ["--format", "json", "pause", "--wait", "--timeout", "5"];
    let error = refusal(&sandbox.run(&command), &command);
    assert_eq!(error["error"], "BadRequest", "got: {error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("already stopped"),
        "got: {error}",
    );

    sandbox.wait("30");

    // `0` means no limit, the way `--timeout 0` does. It used to mean "none of
    // them", answered with an empty list under exit 0 (D079).
    let all = sandbox.json(&["--format", "json", "stack", "--levels", "0"]);
    assert!(
        all["frames"]
            .as_array()
            .is_some_and(|frames| !frames.is_empty()),
        "`--levels 0` means all of them, not none of them: {all}",
    );

    // `big` is two thousand elements. Unasked-for, that is most of an agent's
    // context spent on one variable, and nothing used to say so (D080).
    let big = sandbox.json(&["--format", "json", "eval", "big"]);
    let reference = big["variables_reference"].to_string();

    let capped = sandbox.json(&["--format", "json", "variables", "--reference", &reference]);
    let rows = capped["variables"].as_array().expect("variables").len();
    assert!(rows < 2_000, "the default has to cap something: {rows}");
    assert_eq!(capped["truncated"], true, "and say that it did: {capped}");

    let whole = sandbox.json(&[
        "--format",
        "json",
        "variables",
        "--reference",
        &reference,
        "--max",
        "0",
    ]);
    assert!(
        whole["variables"].as_array().expect("variables").len() > rows,
        "`--max 0` has to lift the cap: {whole}",
    );
    assert_eq!(whole["truncated"], false, "and nothing is missing then");
}

#[test]
fn a_handle_from_a_session_that_has_ended_is_refused_by_the_next_one() {
    // The same failure as a recycled adapter number, one scope out. Handles
    // used to be numbered from `1` per session, and the inspection commands
    // resolve against whichever session is current — so session A's reference
    // was a *live* handle in session B and came back full of another program's
    // variables under exit 0, with nothing to say who had answered (D082).
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("xsess");

    // Session A: stop inside the library crash, where the locals are distinctive.
    let crashes = toolchain.build("library_crash.c");
    sandbox.json(&["--format", "json", "launch", &crashes.to_string_lossy()]);
    let first = sandbox.wait("30");
    let stale = first["locals"]["variables_reference"].to_string();
    let names = |value: &Value| -> Vec<String> {
        value["variables"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row["name"].as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let theirs = names(&sandbox.json(&["--format", "json", "variables", "--reference", &stale]));
    assert!(theirs.contains(&"wanted".to_string()), "got: {theirs:?}");

    // Session B: a different program, in the same daemon.
    sandbox.run(&["disconnect"]);
    let inspects = toolchain.build("inspects.c");
    sandbox.breakpoint("inspects.c", 20);
    sandbox.json(&["--format", "json", "launch", &inspects.to_string_lossy()]);
    let second = sandbox.wait("30");
    assert_eq!(second["state"], "paused", "got: {second}");
    // Mint enough handles that session A's number would have been reachable
    // under the old per-session counter.
    sandbox.json(&["--format", "json", "scopes"]);

    let command = ["--format", "json", "variables", "--reference", &stale];
    let error = refusal(&sandbox.run(&command), &command);
    assert_eq!(error["error"], "StaleHandle", "got: {error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("session that has ended"),
        "and says which kind of stale it is: {error}",
    );
}

#[test]
fn a_window_that_leaves_rows_behind_says_so_however_it_was_narrowed() {
    // `--count 5` on a 2000-element container returned five rows and
    // `truncated: false`, which contradicts the field's documented meaning and
    // stops a client that pages on it. The flag means "there is more than you
    // are seeing", whatever narrowed the list — D072's lesson, applied to this
    // field (D083).
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("window");

    let program = toolchain.build("inspects.c");
    sandbox.breakpoint("inspects.c", 20);
    sandbox.launch(&program);
    sandbox.wait("30");

    let big = sandbox.json(&["--format", "json", "eval", "big"]);
    let reference = big["variables_reference"].to_string();
    let rows = |value: &Value| value["variables"].as_array().expect("variables").len();

    // A caller's own window, well inside the cap, that still leaves rows behind.
    let counted = sandbox.json(&[
        "--format",
        "json",
        "variables",
        "--reference",
        &reference,
        "--count",
        "5",
    ]);
    assert_eq!(rows(&counted), 5, "got: {counted}");
    assert_eq!(
        counted["truncated"], true,
        "five of two thousand is not the whole list: {counted}",
    );

    // The tail: a window that happens to reach the end leaves nothing behind
    // and must not claim it did.
    let tail = sandbox.json(&[
        "--format",
        "json",
        "variables",
        "--reference",
        &reference,
        "--start",
        "1998",
        "--count",
        "50",
        "--max",
        "0",
    ]);
    assert_eq!(
        tail["truncated"], false,
        "nothing was left behind, so nothing is missing: {tail}",
    );

    // The narrower of the two limits wins, and is still reported honestly.
    let both = sandbox.json(&[
        "--format",
        "json",
        "variables",
        "--reference",
        &reference,
        "--count",
        "500",
        "--max",
        "5",
    ]);
    assert_eq!(rows(&both), 5, "the cap binds under a wider count: {both}");
    assert_eq!(both["truncated"], true);
}

#[test]
fn continuing_a_program_that_is_already_running_reports_it_and_invents_no_thread() {
    // Nothing is sent, because there is nothing to ask for (D055). Reporting
    // that as an ordinary resume — with the thread id codelldb answers a
    // `threads` call on a running process with, which is `0`, which is not a
    // thread — was two inventions in one line (D076).
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("running");

    let program = toolchain.build("spins.c");
    // No `--stop-on-entry`: the program is running from the moment it starts.
    sandbox.json(&["--format", "json", "launch", &program.to_string_lossy()]);

    let answer = sandbox.json(&["--format", "json", "continue"]);

    assert_eq!(answer["already_running"], true, "got: {answer}");
    assert!(
        answer["thread_id"].is_null(),
        "nothing was resumed, so no thread was: {answer}",
    );
}

#[test]
fn a_condition_added_to_an_existing_breakpoint_reaches_the_debugger() {
    // The claim being checked is about the *adapter*: re-setting a location
    // that already has a breakpoint has to send the new condition, not the old
    // unconditional one. Before D086 the store returned the breakpoint that
    // was already there, so codelldb was re-sent exactly what it already had
    // and the program stopped on the first iteration.
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("bpcond");
    let program = toolchain.build("inspects.c");

    sandbox.launch(&program);
    let added = sandbox.breakpoint("inspects.c", 14);
    assert_eq!(added["action"], "added", "got: {added}");

    let source = repo_root()
        .join("examples/c-fixtures/inspects.c")
        .display()
        .to_string();
    let updated = sandbox.json(&[
        "--format",
        "json",
        "break",
        &format!("{source}:14"),
        "--condition",
        "i == 3",
    ]);
    assert_eq!(updated["action"], "updated", "got: {updated}");
    assert_eq!(
        updated["applied_to_session"], true,
        "a live session has to be told: {updated}",
    );

    let blob = sandbox.wait("30");
    assert_eq!(blob["state"], "paused", "got: {blob}");
    assert_eq!(blob["frame"]["line"], 14, "got: {blob}");

    let value = sandbox.json(&["--format", "json", "eval", "i"]);
    assert_eq!(
        value["value"], "3",
        "the loop should have run three times before stopping: {value}",
    );
}

#[test]
fn an_adapter_whose_session_ended_is_reaped_rather_than_left_waiting_for_a_disconnect() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("reap");
    let program = toolchain.build("exits.c");

    sandbox.launch(&program);
    let daemon_pid = sandbox.json(&["--format", "json", "status"])["daemon_pid"]
        .as_u64()
        .expect("a daemon pid");
    assert_eq!(sandbox.wait("20")["state"], "exited");

    // codelldb holds its socket open after `terminated` and waits to be
    // disconnected from — so a daemon that only ever read from it kept the
    // adapter, and its ~100MB of LLDB, alive until `lazydap shutdown`. One per
    // session, and an agent loop that launches without disconnecting is every
    // session (D094). Nothing here says `disconnect`: the point is that the
    // daemon does it for itself.
    let survivors = until_gone(|| children_of(daemon_pid, "codelldb"));
    assert!(
        survivors.is_empty(),
        "the adapter outlived the session it was serving: {survivors:?}",
    );
}

#[test]
fn disconnecting_without_terminating_leaves_the_debuggee_running() {
    let (toolchain, _turn) = require_toolchain!();
    let sandbox = Sandbox::new("keep");
    let program = toolchain.build("spins.c");

    sandbox.launch(&program);
    assert_eq!(sandbox.wait("1")["state"], "timeout");
    let debuggee = *processes_matching(&program.display().to_string())
        .first()
        .expect("the fixture is running");

    let answer = sandbox.json(&["--format", "json", "disconnect", "--no-terminate"]);
    assert_eq!(answer["terminated_debuggee"], false, "got: {answer}");

    // The answer said the program was left alone, and until D094 it was
    // not: killing the adapter made the pump read an adapter that had died,
    // and D045's reaper then killed the very process this flag exists to keep.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(
        alive(debuggee),
        "`--no-terminate` promised pid {debuggee} would keep running",
    );

    // It is ours to clean up now — nothing is watching it any more, which is
    // the whole point — and `assert_no_orphans` runs when the sandbox drops.
    let _ = Command::new("kill")
        .arg("-9")
        .arg(debuggee.to_string())
        .output();
    assert!(
        until_gone(|| processes_matching(&program.display().to_string())).is_empty(),
        "the fixture should be gone once it is killed",
    );
}

/// Poll `pids` until it comes back empty, or five seconds have passed.
///
/// Teardown is asynchronous by construction — the daemon answers the client
/// before the adapter has finished going — so a single check after the command
/// returns would be testing the timing rather than the behaviour.
fn until_gone(pids: impl Fn() -> Vec<u32>) -> Vec<u32> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let found = pids();
        if found.is_empty() || std::time::Instant::now() >= deadline {
            return found;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Every process whose command line contains `pattern`, whoever started it.
fn processes_matching(pattern: &str) -> Vec<u32> {
    let output = Command::new("pgrep")
        .args(["-f", pattern])
        .output()
        .expect("run pgrep");

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .is_ok_and(|output| output.status.success())
}
