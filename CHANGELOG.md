# Changelog

All notable changes to lazydap are recorded here.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The **lazydap protocol** is versioned separately from the binary. It is at **v2**; a daemon left running from an older build refuses connections with `VersionMismatch`, and `lazydap shutdown` clears it.

## [0.1.0] — unreleased

First release. Nothing is tagged or published yet; this entry describes what has landed on `main` so far, and it will keep growing until the tag is cut. Rough edges are listed under [Known limitations](#known-limitations) rather than left for you to find.

### Added

**Debugging from the shell.** One binary, `lazydap`, with a subcommand per debug operation: `launch`, `continue`, `step` (aliased `next`), `step-in`, `step-out`, `pause`, `break`, `stack`, `scopes`, `variables`, `eval`, `threads`, `output`, `disconnect`. Paths are relative to your shell, so `lazydap break src/parser.c:142` means what it looks like it means.

**`--wait`, the flag that makes a debugger scriptable.** Every command that moves the program takes it. lazydap blocks until the program reaches a stable state (paused, exited, terminated) and returns one JSON object covering the whole trip: the stop reason, the top frame, which breakpoints were hit, breakpoint state that changed on the way, other threads that stopped, and every line the program printed. `--timeout N` bounds the wait (default 30s, `0` waits forever, `LAZYDAP_TIMEOUT` sets your own default). An adapter that dies mid-wait comes back as a terminated state rather than a hang.

**JSON as the contract.** `--format json` for one object, `jsonl` for streams, `csv` for spreadsheets, `ids` for `xargs`, `table` for your eyes. The format is picked from the tty when you don't say: a pipe gets JSON, a terminal gets a table. Failures print a JSON object on stderr with a stable `error` name and leave stdout empty. Exit codes distinguish a failed command (`1`) from bad arguments (`2`), an unreachable daemon (`3`), and a missing adapter (`4`).

**Breakpoints that outlive the session.** `lazydap break file:line` records a breakpoint in `.lazydap/state.toml` under your project root and re-applies it to every later launch, whether or not a session is running when you set it. `--list`, `--remove`, `--toggle`, selection by `--id` or `--all`, and `--dry-run` on every mutation, using the same selection logic as the real thing. `--condition 'i == 7'` passes an expression to the adapter, so the program only stops when it holds.

**A daemon you never start.** The first command that needs one spawns it, per project root, and every later command in that directory finds it. It holds the debug session, keeps the adapter alive between your invocations, and buffers events so a command that arrives late still sees what happened. `lazydap status`, `lazydap shutdown`, and `lazydap daemon --foreground` when you want to watch it work.

**A terminal UI in the same binary.** Bare `lazydap` on a terminal opens it; in a pipe or a CI job that same command prints help. It shows the source with a marker on the current line and drives the program with `F5`/`c`, `F10`/`n`, `F11` and `Shift-F11`, sending the same requests the CLI sends. `j`/`k`/`<C-d>`/`<C-u>`/`gg`/`G` scroll. `q` leaves the TUI without ending the session, so you can carry on from the shell.

**A live event stream for long-lived clients.** A client can subscribe to the kinds of event it cares about and be pushed frames as they happen, interleaved with replies to its own requests. It is answered with a state snapshot taken when the stream opens. This is what the TUI uses, and it is available to anything else that speaks the socket.

**An agent skill.** `lazydap.skill` at the repository root, packaging usage guidance, a full command reference, output schemas, error codes, and worked debugging sessions. The command reference is generated from lazydap's own argument parser and CI fails when the committed artifact drifts, so an agent never reads about a flag that no longer exists.

**`lazydap doctor`**, which reports whether codelldb is on `PATH`, where your project state lives, and which daemon is answering.

**Shell completions** for bash, zsh, fish, elvish and PowerShell via `lazydap completions <shell>`, generated from the same command tree as `--help`.

**Two codelldb behaviours normalised**, with the adapter's own answer preserved next to lazydap's. `--stop-on-entry` reports `reason: "entry"` and keeps codelldb's `"exception"` in `raw_reason`, because codelldb implements entry-stop with a `SIGSTOP` that LLDB classifies as an exception. `eval` defaults to the `watch` context, because `repl` makes codelldb run an LLDB *command* — under which `eval "x"` reads memory instead of reading your variable. `--context repl` is still there when you want the command interpreter.

### Known limitations

- **codelldb only.** C, C++ and Rust, or anything else LLDB can debug. debugpy, delve and js-debug are planned and not built.
- **One session per project at a time.** A second `launch` while one is live returns `SessionAlreadyActive`. Session ids are in the protocol so multi-session stays possible later.
- **macOS and Linux.** Windows is not a target.
- **The TUI shows source only.** Stack, scopes and breakpoint panes are the next milestones.
- **No config file yet.** `~/.config/lazydap/config.toml` and `.vscode/launch.json` import are specified and not implemented; adapter discovery is `PATH` only.
- **A `variables_reference` is invalidated whenever the program moves.** Re-run `scopes` after each stop.
- **No `attach`, watches, or restart.** Conditional breakpoints work from the CLI; setting one from the TUI does not.
- **The TUI does not reconnect** if the daemon goes away underneath it.
