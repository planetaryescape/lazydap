//! The agent loop against a real delve and real Go debuggees.
//!
//! The M22 question is the M18 question again: not "does delve work" but
//! whether a *third* adapter reaches the same answers through the same
//! commands. So these mirror `wait_debugpy.rs`, which mirrors
//! `wait_codelldb.rs`, against `examples/go-fixtures/`. Where an assertion here
//! differs from its Python twin, that difference is the finding, and it is
//! commented.
//!
//! Two things are genuinely new here and both are asserted below rather than
//! only written up:
//!
//! - **The debuggee's output only exists because lazydap asks for it.** delve's
//!   default puts it on the adapter's own stdout, where nothing reads it
//!   (quirk 2). Every `captured_output` assertion in this file is a regression
//!   test for that one launch argument.
//! - **A Go debuggee is not the fixture.** `mode: "debug"` compiles the source
//!   and runs the *binary*, so the orphan check cannot look for the `.go` path
//!   the way the Python suite looks for its `.py`. See [`strays`].
//!
//! Every test skips, loudly, when there is no `dlv` on `PATH`, so a machine
//! without one still gets a green `cargo test`.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const LAZYDAP: &str = env!("CARGO_BIN_EXE_lazydap");

/// One debug session at a time, across the whole file.
///
/// The same discipline the other two suites explain, for the same reason:
/// `cargo test` runs a file's tests in parallel, the launch handshake has a
/// fifteen-second deadline, and delve pays a cost neither of the others does —
/// `mode: "debug"` runs the Go compiler before the program starts, so a
/// contended machine can spend most of that deadline in `go build`.
///
/// Separate from the other files' mutexes, necessarily: a `static` is
/// per-test-binary, so the three suites cannot share one however it is
/// written. What keeps them from overlapping is that each takes turns within
/// itself and each cleans up after itself.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Claim the machine and check it can run this at all.
macro_rules! require_dlv {
    () => {{
        // Held for the rest of the test: the guard is returned alongside the
        // binary so it lives exactly as long as the test body does.
        let guard = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match find_dlv() {
            Some(dlv) => (dlv, guard),
            None => {
                eprintln!(
                    "skipping {}: needs a `dlv` on PATH with DAP support \
                     (`go install github.com/go-delve/delve/cmd/dlv@latest`, \
                     and put $(go env GOPATH)/bin on PATH)",
                    std::thread::current().name().unwrap_or("this test"),
                );
                return;
            }
        }
    }};
}

/// The first `dlv` on `PATH` that has the `dap` subcommand.
///
/// The same rule `crate::adapter::usable` applies, on purpose: a test that
/// found its adapter a different way could pass while the product's own
/// discovery was picking a different one — or rejecting it.
fn find_dlv() -> Option<PathBuf> {
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        let candidate = dir.join("dlv");
        if !candidate.is_file() {
            continue;
        }
        let usable = Command::new(&candidate)
            .args(["help", "dap"])
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
        if usable {
            return Some(candidate);
        }
    }
    None
}

/// Where a fixture lives. Nothing is built by hand: delve compiles the source
/// as part of the launch.
fn fixture(name: &str) -> PathBuf {
    repo_root().join("examples/go-fixtures").join(name)
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
    /// The `lazydap-delve-` files already in the temp directory when this
    /// sandbox started — another machine's, another worktree's, or debris a
    /// crash left behind. The file-leak check subtracts these so it flags only
    /// what *this* session created, never pre-existing clutter.
    preexisting_artifacts: std::collections::HashSet<PathBuf>,
}

impl Sandbox {
    /// Terse names on purpose: a Unix socket path has about a hundred bytes,
    /// and lazydap refuses to bind one that overruns it.
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let instance = format!("{label}{}-{unique}", std::process::id());
        let root = PathBuf::from("/tmp").join(format!("lzdgo-{instance}"));

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
            preexisting_artifacts: artifact_files(),
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
    /// `.go`, and a test that passed it explicitly would not be testing that.
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

        // Not while the test is already failing. A second panic during unwind
        // aborts the process, which takes the *first* panic's message with it
        // — so a stray check meant to explain a failure would instead hide
        // one. The strays still get reported, just as output rather than as an
        // assertion nobody can read.
        let survivors = self.strays();
        if std::thread::panicking() {
            if !survivors.is_empty() {
                eprintln!("strays left behind by a failing test: {survivors:?}");
            }
            return;
        }
        assert!(
            survivors.is_empty(),
            "a Go debuggee or its compiled binary outlived its session — {}. The \
             adapter died without stopping it and nothing reaped it; see D045.",
            survivors.join(", "),
        );
    }
}

impl Sandbox {
    /// Anything of ours left behind: processes still running, and files still
    /// on disk.
    ///
    /// Two kinds of leak, because delve produces two. A **process** leak — the
    /// Python suite finds these by the fixture path, but nothing runs a `.go`
    /// by name: `mode: "debug"` compiles the source and runs the *result*, so a
    /// leaked Go debuggee is in the process table as the compiled binary, under
    /// the `lazydap-delve-` prefix lazydap gives it (quirk 5). A **file** leak —
    /// the compiled binary itself, left on disk if the adapter died before
    /// deleting it (finding 4); a process-only check cannot see this. Both are
    /// strays, so the adapter-kill test's poll-until-clean loop and the `Drop`
    /// assertion cover files as well as processes with no extra machinery.
    ///
    /// Files present when this sandbox started are subtracted: another run's
    /// debris is not this test's leak.
    fn strays(&self) -> Vec<String> {
        let fixtures = repo_root().join("examples/go-fixtures");
        let patterns = [fixtures.display().to_string(), "lazydap-delve-".to_string()];

        let mut strays: Vec<String> = patterns
            .iter()
            .flat_map(|pattern| {
                let output = Command::new("pgrep")
                    .args(["-fl", pattern])
                    .output()
                    .expect("run pgrep");
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(|line| line.trim().to_string())
                    .filter(|line| !line.is_empty())
                    // `dlv dap` itself is not a stray while the daemon that owns
                    // it is still shutting down; only the debuggee and a
                    // compiled binary are. An adapter with no session is caught
                    // by the daemon's own teardown, which `Drop` has already run.
                    .filter(|line| !line.contains("dlv dap"))
                    .filter(|line| could_be_a_debuggee(line))
                    .collect::<Vec<_>>()
            })
            .collect();

        strays.extend(
            self.new_artifacts()
                .iter()
                .map(|path| format!("file {}", path.display())),
        );
        strays
    }

    /// The `lazydap-delve-` files that have appeared since this sandbox started
    /// — the ones *this* session is responsible for.
    fn new_artifacts(&self) -> Vec<PathBuf> {
        artifact_files()
            .difference(&self.preexisting_artifacts)
            .cloned()
            .collect()
    }
}

/// Whether a `pgrep -fl` line could be a Go debuggee at all.
///
/// `pgrep -f` matches a whole command line, machine-wide, so it also matches
/// anything that merely *names* the pattern — including the shell running a
/// `pgrep` for these very words. That is not hypothetical: a concurrent session
/// checking this machine for strays failed this suite, reported as a leaked
/// debuggee whose "command line" was somebody's `pgrep -fl
/// "lazydap-delve-|go-fixtures|dlv "`. A shell and a process-search tool cannot
/// be a debuggee, whatever they have in their arguments.
fn could_be_a_debuggee(line: &str) -> bool {
    // `pgrep -l` prints `<pid> <command line>`.
    let Some((_, command)) = line.split_once(' ') else {
        return false;
    };
    let program = command.split_whitespace().next().unwrap_or_default();
    let program = program.rsplit('/').next().unwrap_or(program);
    !matches!(
        program,
        "pgrep" | "grep" | "ps" | "sh" | "bash" | "zsh" | "fish"
    )
}

/// Every `lazydap-delve-` file currently directly in the temp directory.
///
/// A set so a sandbox can subtract the ones that were already there. The name
/// carries the daemon's pid, not the test's, so there is nothing tighter than
/// the prefix to key on — the baseline subtraction is what makes that safe.
fn artifact_files() -> std::collections::HashSet<PathBuf> {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return std::collections::HashSet::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("lazydap-delve-"))
        })
        .map(|entry| entry.path())
        .collect()
}

/// Children of `parent` whose command line contains `pattern`.
///
/// Scoped to this daemon's children so a suite running in another worktree
/// keeps its adapters.
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
fn a_go_program_is_launched_under_delve_without_being_told_to() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("pick");

    let launched = sandbox.launch(&fixture("exits.go"));

    // delve reports a stop-on-entry stop as `entry` natively, as debugpy does
    // and codelldb does not, so D033's renaming has nothing to do and
    // `raw_reason` stays null.
    assert_eq!(launched["state"], "paused", "got: {launched}");
    assert_eq!(launched["reason"], "entry", "got: {launched}");
    assert_eq!(
        launched["raw_reason"],
        Value::Null,
        "nothing was renamed, so nothing to disclose: {launched}",
    );

    let status = sandbox.json(&["--format", "json", "status"]);
    assert_eq!(
        status["session"]["adapter"], "delve",
        "a `.go` picks delve with no --adapter: {status}",
    );
}

#[test]
fn continuing_to_a_breakpoint_reports_where_and_why_it_stopped() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("bp");

    sandbox.launch(&fixture("exits.go"));
    let added = sandbox.breakpoint("exits.go", 11);
    assert_eq!(added["breakpoints"][0]["verified"], true, "got: {added}");

    let blob = sandbox.wait("30");

    assert_eq!(blob["state"], "paused", "got: {blob}");
    assert_eq!(blob["reason"], "breakpoint", "got: {blob}");
    assert_eq!(blob["frame"]["line"], 11, "got: {blob}");
    assert_eq!(blob["frame"]["name"], "main.main", "got: {blob}");

    // Unlike debugpy, delve *does* name the breakpoint it hit — its `stopped`
    // event carries `hitBreakpointIds`, the way codelldb's does. An agent that
    // branches on which breakpoint was hit works here and does not under
    // debugpy; see `docs/reference/delve-quirks.md`.
    assert_eq!(
        blob["hit_breakpoint_ids"],
        serde_json::json!([1]),
        "delve names the breakpoint it stopped on: {blob}",
    );
}

/// Launch without `--stop-on-entry`, then `continue --wait` — the ordinary way
/// to reach a first breakpoint, and the case that caught debugpy out at M18.
#[test]
fn continuing_a_program_that_is_already_running_reaches_the_breakpoint() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("run");

    sandbox.breakpoint("exits.go", 11);
    let launched = sandbox.json(&[
        "--format",
        "json",
        "launch",
        &fixture("exits.go").to_string_lossy(),
    ]);
    assert_eq!(launched["state"], "running", "got: {launched}");

    let blob = sandbox.wait("30");

    assert_eq!(blob["state"], "paused", "got: {blob}");
    assert_eq!(blob["reason"], "breakpoint", "got: {blob}");
    assert_eq!(blob["frame"]["line"], 11, "got: {blob}");
}

#[test]
fn a_program_that_finishes_reports_its_exit_code() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("exit");

    sandbox.launch(&fixture("exits.go"));
    let blob = sandbox.wait("30");

    assert_eq!(blob["state"], "exited", "got: {blob}");
    assert_eq!(blob["exit_code"], 0, "got: {blob}");
    // The `outputMode: "remote"` regression test. Without that launch argument
    // this is empty and the program still prints — to delve's stdout, where
    // nothing reads it (quirk 2).
    assert!(
        output_texts(&blob).iter().any(|text| text.contains("x=5")),
        "the debuggee's stdout must survive the run: {blob}",
    );
}

#[test]
fn a_program_that_never_stops_times_out_without_ending_the_session() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("spin");

    sandbox.launch(&fixture("spins.go"));
    let blob = sandbox.wait("3");

    assert_eq!(blob["state"], "timeout", "got: {blob}");

    // The session is still there and still usable — a timeout is this call
    // giving up, not the program being over.
    let status = sandbox.json(&["--format", "json", "status"]);
    assert_eq!(status["session"]["state"], "running", "got: {status}");
}

/// Finding 4, the case that slipped past the other tests: a program that
/// exited, then `shutdown` with no `disconnect` first.
///
/// delve deletes its compiled binary when it handles a `disconnect`, and an
/// exited session used to send none — the daemon read on until `shutdown` killed
/// the adapter, so the binary leaked. Since D-WP1-1 the pump disconnects an
/// adapter whose session has ended, which gives delve the chance to delete it
/// *before* shutdown; either way, nothing of this session may be on disk once
/// the daemon is gone. The other tests miss this because the sandbox always
/// disconnects in `Drop` before it shuts down, which cleans the file the other
/// way. This one shuts down directly.
#[test]
fn the_compiled_binary_is_removed_when_an_exited_session_is_shut_down() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("artifact");

    sandbox.launch(&fixture("exits.go"));
    let blob = sandbox.wait("30");
    assert_eq!(blob["state"], "exited", "got: {blob}");

    // `mode: "debug"` compiles exactly one binary, and whether it is still on
    // disk right now is a race with delve acting on the disconnect the ended
    // session sent it — `exits.go` can finish during its own launch, in which
    // case the wind-down happens before this line. So the assertion is about
    // what must never survive rather than about the race, and it is made after
    // the shutdown, where both branches have to agree: nothing of this session
    // is left in the temp directory.
    assert!(
        sandbox.new_artifacts().len() <= 1,
        "debug mode compiles one binary: {:?}",
        sandbox.new_artifacts(),
    );

    // Shut down with no prior disconnect — the path that leaked.
    sandbox.run(&["shutdown"]);
    for _ in 0..30 {
        if sandbox.new_artifacts().is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    unreachable!(
        "delve's compiled binary outlived the daemon: {:?}",
        sandbox.new_artifacts(),
    );
}

#[test]
fn a_lot_of_output_arrives_in_order() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("chat");

    sandbox.launch(&fixture("chatty.go"));
    sandbox.breakpoint("chatty.go", 12);
    let blob = sandbox.wait("30");

    assert_eq!(blob["state"], "paused", "got: {blob}");

    // Ordering, not volume: whatever the prints are chunked into, "line 0"
    // must precede "line 199".
    let joined = output_texts(&blob).join("");
    let first = joined.find("line 0").expect("the first line");
    let last = joined.find("line 199").expect("the last line");
    assert!(first < last, "output arrived out of order: {joined}");
}

/// D045 for Go: the adapter is killed mid-wait, and nothing is left behind.
///
/// What this shows and does not, stated plainly. It shows the *outcome*: the
/// wait ends as `adapter_died` rather than sitting until its timeout (D022),
/// and no debuggee survives. It does **not** discriminate lazydap's reap from
/// delve's own cleanup — observed on delve 1.27.0, the debuggee is gone within
/// a second of the adapter being killed, so the pid is already dead when the
/// reap looks at it. delve behaves as debugpy does here, not as codelldb does.
#[test]
fn a_killed_adapter_takes_its_go_debuggee_with_it() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("dead");

    sandbox.launch(&fixture("spins.go"));
    let daemon_pid = sandbox.json(&["--format", "json", "status"])["daemon_pid"]
        .as_u64()
        .expect("a daemon pid");

    // Kill the adapter while a wait is in flight — nothing sends `terminated`,
    // which is the case D022 exists for, and nothing stops the debuggee, which
    // is the case D045 exists for.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(700));
        for pid in children_matching(daemon_pid, "dlv") {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).output();
        }
    });

    let blob = sandbox.wait("30");
    assert_eq!(blob["state"], "adapter_died", "got: {blob}");
    assert!(
        blob["elapsed_ms"].as_u64().unwrap_or(u64::MAX) < 30_000,
        "it must not have waited out the timeout: {blob}",
    );

    // `spins.go` sleeps rather than spinning, so an orphan costs no CPU and
    // would be that much easier to miss. The compiled binary must go too — the
    // adapter died before deleting it (finding 4). Give the cleanup a moment to
    // land; `strays` covers both the process and the file.
    for _ in 0..20 {
        if sandbox.strays().is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    unreachable!(
        "the debuggee or its compiled binary outlived its adapter — {}",
        sandbox.strays().join(", "),
    );
}

/// Where the three adapters differ most, so it is asserted rather than assumed.
///
/// - **codelldb**: a segfault is a signal the debugger sees whether or not
///   anybody asked, and it pauses.
/// - **debugpy**: an uncaught exception is only a stop if the client asked for
///   one with `setExceptionBreakpoints`, and lazydap sends no filters — so the
///   program dies unattended, exit code 1, no pause.
/// - **delve**: pauses, with no filters sent either. Its DAP server applies its
///   own `unrecovered-panic` default server-side, so the stop happens because
///   delve decided it should.
///
/// An agent that learned "a crash means `state: exited`" from the Python suite
/// would be wrong here, which is exactly why this is a test and a quirk entry
/// rather than a footnote.
#[test]
fn an_unrecovered_panic_pauses_rather_than_ending_the_program() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("panic");

    sandbox.launch(&fixture("crashes.go"));
    let blob = sandbox.wait("30");

    assert_eq!(blob["state"], "paused", "got: {blob}");
    assert_eq!(blob["reason"], "exception", "got: {blob}");
    assert!(
        output_texts(&blob)
            .iter()
            .any(|text| text.contains("about to fail")),
        "what the program managed to say before panicking must survive: {blob}",
    );
}

/// delve's entry stop happens before the Go runtime has scheduled anything, so
/// there is no goroutine to describe and `stackTrace` fails outright.
///
/// Asserted rather than worked around. The alternatives were to hide it — an
/// empty stack would say "no frames" where the truth is "not yet" — or to skip
/// the entry stop for Go, which would take a working `--stop-on-entry` away.
/// An agent that wants a stack should continue to a breakpoint first, which is
/// what every other test in this file does. See `docs/reference/delve-quirks.md`,
/// quirk 6.
#[test]
fn the_entry_stop_has_no_goroutine_to_take_a_stack_of() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("entry");

    sandbox.launch(&fixture("exits.go"));

    let refused = sandbox.run(&["--format", "json", "stack"]);
    assert!(
        !refused.status.success(),
        "an impossible stack must be an error, not an empty one: {}",
        String::from_utf8_lossy(&refused.stdout),
    );

    // The thread delve reports at this point is a placeholder it names
    // `Dummy`. Worth pinning: if it ever becomes a real goroutine, the quirk
    // is gone and this test is how we find out.
    let threads = sandbox.json(&["--format", "json", "threads"]);
    assert_eq!(threads["threads"][0]["name"], "Dummy", "got: {threads}");
}

/// A Go program that is already built runs under `mode: "exec"`.
///
/// Which mode to use is read off the extension, so this also covers the
/// inference: a binary has none, and lands on `exec`. It needs `--adapter
/// delve` because a bare compiled binary is otherwise codelldb's by default —
/// which is the behaviour M22 deliberately kept.
#[test]
fn an_already_compiled_go_binary_runs_under_exec_mode() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("exec");

    let binary = std::env::temp_dir().join(format!("lazydap-exits-{}", std::process::id()));
    let built = Command::new("go")
        .args(["build", "-gcflags", "all=-N -l", "-o"])
        .arg(&binary)
        .arg(fixture("exits.go"))
        .output()
        .expect("run go build");
    assert!(
        built.status.success(),
        "go build failed: {}",
        String::from_utf8_lossy(&built.stderr),
    );

    let launched = sandbox.json(&[
        "--format",
        "json",
        "launch",
        &binary.to_string_lossy(),
        "--adapter",
        "delve",
        "--stop-on-entry",
    ]);
    assert_eq!(launched["state"], "paused", "got: {launched}");
    assert_eq!(launched["reason"], "entry", "got: {launched}");

    let blob = sandbox.wait("30");
    assert_eq!(blob["state"], "exited", "got: {blob}");
    assert_eq!(blob["exit_code"], 0, "got: {blob}");
    assert!(
        output_texts(&blob).iter().any(|text| text.contains("x=5")),
        "got: {blob}",
    );

    let _ = std::fs::remove_file(&binary);
}

#[test]
fn variables_and_expressions_read_the_paused_frame() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("eval");

    sandbox.launch(&fixture("exits.go"));
    sandbox.breakpoint("exits.go", 11);
    sandbox.wait("30");

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
        "delve names the frame's own scope `Locals`, as debugpy does: {scopes}",
    );

    let evaluated = sandbox.json(&["--format", "json", "eval", "x * 3"]);
    assert_eq!(evaluated["value"], "15", "got: {evaluated}");

    // An expression Go cannot evaluate is the adapter's refusal, reported as
    // one rather than as an empty success.
    let refused = sandbox.run(&["--format", "json", "eval", "nope"]);
    assert!(
        !refused.status.success(),
        "an undefined name must not look like a value: {}",
        String::from_utf8_lossy(&refused.stdout),
    );
}

#[test]
fn an_adapter_whose_session_ended_is_reaped_rather_than_left_waiting_for_a_disconnect() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("reap");

    sandbox.launch(&fixture("exits.go"));
    let daemon_pid = sandbox.json(&["--format", "json", "status"])["daemon_pid"]
        .as_u64()
        .expect("a daemon pid");
    assert_eq!(sandbox.wait("30")["state"], "exited");

    // The third adapter, the same finding: delve holds its socket open after
    // `terminated` too, so nothing but `lazydap shutdown` collected one. It
    // also has a second thing to lose by being killed rather than disconnected
    // — the binary it compiled, which it deletes on its own way out (quirk 5) —
    // and the sandbox's `Drop` checks for that file as well (D-WP1-1).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let survivors = loop {
        let found = children_matching(daemon_pid, "dlv");
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
fn disconnecting_without_terminating_says_so_honestly_when_delve_cannot_detach() {
    let (_dlv, _turn) = require_dlv!();
    let sandbox = Sandbox::new("keep");

    sandbox.launch(&fixture("spins.go"));
    assert_eq!(sandbox.wait("1")["state"], "timeout");

    // delve does not advertise DAP's `supportTerminateDebuggee`, and it means
    // it: the debuggee is delve's own child and dies with it whatever the
    // request said. Reporting `terminated_debuggee: false` told a caller their
    // program was still running when it had been dead for eighty milliseconds
    // (D-WP1-2).
    let started = std::time::Instant::now();
    let answer = sandbox.json(&["--format", "json", "disconnect", "--no-terminate"]);

    assert_eq!(
        answer["terminated_debuggee"], true,
        "the program does die, and the answer has to say so: {answer}",
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "nothing here is worth waiting on: {:?}",
        started.elapsed(),
    );
    // The sandbox's `Drop` checks the process *and* the compiled binary.
}
