//! Making sure the user's program dies with its debugger.
//!
//! # The bug this exists for
//!
//! codelldb spawns the debuggee as its own child, and reaps it when it shuts
//! down cleanly. When it does *not* shut down cleanly — a crash, an OOM kill,
//! a `kill -9` — it never gets the chance: the debuggee is reparented to init
//! and keeps running, with nothing left that knows it is a debuggee. A program
//! stopped at a breakpoint is suspended and stays that way; one that was
//! running busy-loops forever.
//!
//! The daemon's adapter-death path killed the adapter process it owns and
//! stopped there, so the debuggee was nobody's problem. Found by counting: 46
//! orphaned test fixtures had accumulated across worktrees, one per run of the
//! suite that SIGKILLs an adapter mid-wait. The test was only the reproduction
//! — the same thing happens to a real user's program whenever codelldb crashes.
//!
//! # Why the pid is scraped rather than asked for
//!
//! DAP defines a `process` event carrying `systemProcessId`, which would be
//! the right way to learn this. codelldb does not send it: the string does not
//! appear anywhere in its binary, and a full launch-to-exit event stream
//! contains `output`, `initialized`, `module`, `continued`, `exited` and
//! `terminated` and nothing else. What it does do is print
//! `Launched process 1234 from '/path/to/program'` to the console category,
//! and that is where the pid comes from. See `docs/reference/codelldb-quirks.md`
//! quirk 9.
//!
//! That is fragile, and it is allowed to be: this is best-effort cleanup. A
//! parse that fails leaves things exactly as they were before this module
//! existed, and says so in the log rather than failing a session.

use std::path::{Path, PathBuf};
use tokio::process::Command;

/// A process we started and are therefore responsible for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Debuggee {
    pub pid: u32,
    /// What we launched, kept for the identity check below.
    pub program: PathBuf,
}

impl Debuggee {
    /// Kill it, if it is still running and still the process we think it is.
    ///
    /// Answers what happened, in words meant for the `detail` of a synthesised
    /// ending — a user whose adapter crashed should be told whether their
    /// program went with it.
    pub async fn reap(&self) -> Option<String> {
        match self.identify().await {
            Identity::Gone => {
                tracing::debug!(
                    target: "daemon.session",
                    pid = self.pid,
                    "the debuggee had already exited",
                );
                None
            }
            // Pid reuse. A daemon can outlive many programs, and killing
            // whatever happens to hold the number now would be far worse than
            // leaking the one we were looking for.
            Identity::SomebodyElse(command) => {
                tracing::warn!(
                    target: "daemon.session",
                    pid = self.pid,
                    %command,
                    expected = %self.program.display(),
                    "not killing a pid that has been reused",
                );
                None
            }
            Identity::Ours => match kill(self.pid).await {
                true => {
                    tracing::warn!(
                        target: "daemon.session",
                        pid = self.pid,
                        program = %self.program.display(),
                        "the adapter died without stopping the debuggee; killed it",
                    );
                    Some(format!(
                        "the debuggee (pid {}) was left running and has been killed",
                        self.pid,
                    ))
                }
                false => {
                    tracing::warn!(
                        target: "daemon.session",
                        pid = self.pid,
                        "could not kill the orphaned debuggee",
                    );
                    Some(format!(
                        "the debuggee (pid {}) was left running and could not be killed",
                        self.pid,
                    ))
                }
            },
        }
    }

    /// Whether the pid still belongs to the program we launched.
    async fn identify(&self) -> Identity {
        let Ok(output) = Command::new("ps")
            .args(["-o", "command=", "-p", &self.pid.to_string()])
            .output()
            .await
        else {
            // No `ps` is not a reason to start killing pids on faith.
            return Identity::Gone;
        };
        let command = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if command.is_empty() {
            return Identity::Gone;
        }
        match owns(&command, &self.program) {
            true => Identity::Ours,
            false => Identity::SomebodyElse(command),
        }
    }
}

enum Identity {
    Gone,
    Ours,
    SomebodyElse(String),
}

/// Whether a `ps` command line is the program at `path`.
///
/// Two shapes have to match, because two adapters start programs differently:
///
/// - **The program is the command.** codelldb execs the binary, so `ps` shows
///   `/tmp/spins --loud`. A prefix match covers it: the debuggee's arguments
///   follow its path, and a program launched with arguments is still ours.
/// - **The program is an argument.** debugpy runs a script under an
///   interpreter, so `ps` shows `/opt/homebrew/bin/python3 /tmp/main.py`. The
///   path we launched is in there, but not at the front.
///
/// So the path is also looked for as a **whole token** anywhere in the command.
/// Whole-token, not substring: `/tmp/spins-other` contains `/tmp/spins`, and
/// killing it would be killing a stranger — which is the failure this check
/// exists to prevent, and is worse than the leak it exists to fix.
///
/// A path containing a space cannot be found this way, since `ps` gives no way
/// to tell that space from an argument separator. The prefix match still
/// covers the codelldb shape for such a path; the interpreter shape falls
/// through to "not ours" and leaks, which is the safe direction to be wrong in.
fn owns(command: &str, path: &Path) -> bool {
    let program = path.to_string_lossy();
    if command == program || command.starts_with(&format!("{program} ")) {
        return true;
    }
    command
        .split_whitespace()
        .any(|argument| argument == program)
}

/// `kill -9`, through the command rather than a `libc` call.
///
/// Adapter death is rare enough that one process spawn costs nothing, and it
/// keeps a new dependency out of the tree for a path this small.
async fn kill(pid: u32) -> bool {
    Command::new("kill")
        .args(["-9", &pid.to_string()])
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_program_path_is_recognised_as_ours() {
        assert!(owns("/tmp/spins", Path::new("/tmp/spins")));
    }

    #[test]
    fn a_program_with_arguments_is_still_ours() {
        assert!(owns("/tmp/spins --loud 3", Path::new("/tmp/spins")));
    }

    #[test]
    fn a_different_program_sharing_a_prefix_is_not_ours() {
        // The reason the space matters: `/tmp/spins-other` starts with
        // `/tmp/spins`, and killing it would be killing a stranger.
        assert!(!owns("/tmp/spins-other", Path::new("/tmp/spins")));
        assert!(!owns("/usr/bin/vim", Path::new("/tmp/spins")));
    }

    /// The debugpy shape. Without this, adapter death classifies every Python
    /// debuggee as somebody else's process and orphans it — the exact bug D045
    /// exists to prevent, reintroduced by the second adapter.
    #[test]
    fn a_script_run_under_an_interpreter_is_ours() {
        // Captured from `ps -o command=` for a real debugpy session.
        assert!(owns(
            "/opt/homebrew/bin/python3 /tmp/py-fixtures/spins.py",
            Path::new("/tmp/py-fixtures/spins.py"),
        ));
        // And with arguments on both sides of it.
        assert!(owns(
            "/usr/bin/python3 -X frozen_modules=off /tmp/main.py --loud",
            Path::new("/tmp/main.py"),
        ));
    }

    #[test]
    fn an_interpreter_running_a_different_script_is_not_ours() {
        assert!(!owns(
            "/opt/homebrew/bin/python3 /tmp/somebody-elses.py",
            Path::new("/tmp/main.py"),
        ));
        // Substring, not a whole argument: `/tmp/main.py.bak` is another file.
        assert!(!owns(
            "/opt/homebrew/bin/python3 /tmp/main.py.bak",
            Path::new("/tmp/main.py"),
        ));
    }

    #[tokio::test]
    async fn a_pid_that_is_not_running_is_left_alone() {
        // Pid 0 is never a user process, so `ps` finds nothing for it.
        let debuggee = Debuggee {
            pid: 0,
            program: PathBuf::from("/tmp/nothing-here"),
        };
        assert_eq!(debuggee.reap().await, None);
    }

    #[tokio::test]
    async fn a_pid_that_has_been_reused_is_not_killed() {
        // This process is certainly alive and certainly not the program named.
        let debuggee = Debuggee {
            pid: std::process::id(),
            program: PathBuf::from("/tmp/definitely-not-this-test-binary"),
        };
        assert_eq!(debuggee.reap().await, None, "and we are still running");
    }

    /// The debugpy shape, end to end against a real process and a real `ps`.
    ///
    /// The unit tests above check the string comparison; this checks that the
    /// comparison is fed what a running interpreter actually looks like. It is
    /// here rather than in `wait_debugpy.rs` because it cannot be observed
    /// there: debugpy's launcher kills the debuggee itself when the adapter
    /// socket drops, so by the time the reap looks, the pid is already gone
    /// and every branch of this check returns the same answer. The identity
    /// check is still wrong without the fix — it just has nothing to be wrong
    /// about while debugpy is cleaning up after itself, which is not something
    /// to rely on.
    #[tokio::test]
    async fn a_script_running_under_an_interpreter_is_recognised_and_killed() {
        let script = std::env::temp_dir().join(format!("lazydap-reap-{}.py", std::process::id()));
        std::fs::write(&script, "import time\ntime.sleep(60)\n").expect("write the script");

        let Ok(mut child) = tokio::process::Command::new("python3").arg(&script).spawn() else {
            eprintln!("skipping: needs python3 on PATH");
            let _ = std::fs::remove_file(&script);
            return;
        };
        let pid = child.id().expect("a pid");

        let detail = Debuggee {
            pid,
            program: script.clone(),
        }
        .reap()
        .await;

        // Unconditionally, before asserting: if the reap declined, the child
        // is still sleeping, and waiting on it would turn a failed assertion
        // into a minute of nothing.
        let _ = child.start_kill();
        let _ = child.wait().await;
        let _ = std::fs::remove_file(&script);

        // `ps` shows `python3 /tmp/lazydap-reap-N.py`, so the program we
        // launched is an argument rather than the command. A prefix match
        // calls that somebody else's process and leaves it running.
        let detail = detail.expect("an interpreter-run debuggee is still ours to reap");
        assert!(detail.contains("killed"), "got: {detail}");
    }

    #[tokio::test]
    async fn a_live_debuggee_of_ours_is_killed_and_reported() {
        let mut child = tokio::process::Command::new("/bin/sleep")
            .arg("60")
            .spawn()
            .expect("spawn a stand-in debuggee");
        let pid = child.id().expect("a pid");

        let detail = Debuggee {
            pid,
            program: PathBuf::from("/bin/sleep"),
        }
        .reap()
        .await
        .expect("it should say it killed it");

        assert!(detail.contains(&pid.to_string()), "got: {detail}");
        assert!(detail.contains("killed"), "got: {detail}");
        // Reaped by us, so the child does not linger as a zombie either.
        let _ = child.wait().await;
    }
}
