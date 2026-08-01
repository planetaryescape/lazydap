# Changelog

All notable changes to lazydap are recorded here.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The **lazydap protocol** is versioned separately from the binary. It is at **v7**; a daemon left running from an older build refuses connections with `VersionMismatch`, and `lazydap shutdown` clears it — which the TUI now does for itself.

## [Unreleased]

### Added

**Watch expressions.** `lazydap watch add <expression>`, `watch list` and `watch remove`, with `--dry-run` on both mutations and `--format ids` for piping. A watch is project state, recorded in `.lazydap/state.toml` beside your breakpoints: you can set one before anything is running, and it is still there after the daemon has gone. What it *evaluates to* is not recorded, because that is only true while the program is sitting still.

**A watches pane in the TUI.** `Tab` reaches it, `a` adds an expression, `dd` removes the selected one. Every expression is re-evaluated each time the program stops, and again when you select a different stack frame — so the watches and the scopes pane are always talking about the same function. An expression that is out of scope keeps its row and shows the adapter's complaint, because the same expression is usually back in scope a few steps later.

**A REPL pane in the TUI.** `Tab` reaches it, type an expression and press `<CR>`; the answer appears under the line that asked for it. It sends the same request `lazydap eval` sends, in the same `watch` context and for the same reason — `repl` context makes codelldb run an LLDB *command*, so `x` becomes a memory read rather than your variable. Adapter commands are still one keystroke away: a line starting with `/` goes to the adapter, so `/bt` is a backtrace. `<C-p>` and `<C-n>` walk the history, which lasts for the session.

### Changed

**Protocol v4 → v5.** The watch requests and the `WatchUpdated` event. A `Request` variant an older daemon does not know is not a soft failure — it cannot decode the frame at all, so it never reaches the version field it would have refused on. The bump turns "old daemon still running" into the `VersionMismatch` that `lazydap shutdown` clears and auto-spawn replaces.

**Protocol v5 → v7.** v6 added the `delve` adapter — a new `AdapterKind` variant, which an older daemon cannot decode at all. v7 changed four things about what the daemon reports: `threads` may omit a thread's `name`, the `--wait` blob and the `Stopped` event gained `adapter_thread_id`, `capabilities` gained `supports_variable_paging`, and a variable gained `evaluate_name`.

**`lazydap variables --start` and `--count` now work against every adapter.** codelldb does not implement DAP's variable paging and silently ignored both, so `--start 100 --count 5` on a 2000-element array returned all of it from `[0]`. lazydap applies the window itself when the adapter has not claimed it. `--filter` is passed through to the debugger and *not* emulated: nothing on the wire says which children are indexed, and guessing from how a name is spelled would return the wrong rows against an adapter that spells them differently.

### Fixed

**`lazydap pause --wait` reported a crash.** codelldb implements `pause` by signalling the process, and LLDB calls a signal stop an exception — so asking a program to stop came back as `"reason": "exception"`, which reads as a segfault. It is now `"reason": "pause"`, with the adapter's own word kept in `raw_reason`. The same fix covers a `pause` racing a step: both are tracked, so neither answer is lost.

**`lazydap threads` invented a thread name.** Asked while the program was running, codelldb replies with one nameless thread `0` — a placeholder. lazydap filled that in as `"thread 0"`, which reads like a real answer about a real thread. `name` is now absent when the debugger gave none.

**`lazydap step --thread` reported a different thread.** codelldb answers a step aimed at one thread by naming whichever thread it had selected before — the one that did *not* move. That thread was reported, and became the default for the next `lazydap stack`. The blob now names the thread that was asked to step, with the debugger's answer kept in `adapter_thread_id`.

**`output_truncated` meant two different things and admitted to neither.** A run that outran the output cap kept accepting later output that still fit, so `captured_output` was a *splice* — hundreds of lines missing from the middle with the tail glued onto the cut. Separately, a program chatty enough to overrun the session's event buffer between two commands lost the *beginning* of its output and the flag stayed `false`. Both now set it, `dropped_events` says how many events were lost, and what you keep is a genuine prefix or a flagged suffix.

**`lazydap eval` returned errors as values with exit 0.** codelldb answers an expression it could not evaluate with a *successful* response whose result is the error text. Those now fail properly. The check is deliberately narrow — the literal `<error:` prefix — because `<last operation failed>` is a summary string a real program can have, and failing a legitimate value is worse than the bug. An unreadable address still comes back as `<read memory ... failed ...>` with exit 0; that gap is documented rather than papered over.

**Variables carry `evaluate_name`.** The debugger's own answer to "what expression names this row", and the only reliable way to turn a row called `[100]` into something `lazydap eval` accepts.

**A Python frame's `source.name` is no longer blank.** debugpy sends only `path` where codelldb and delve send both, so `frame.source.name` was missing for one language out of three. It is filled from the path lazydap already had.

**The TUI no longer writes its own log lines across its panes.** Every other command logs to stderr, which is right for one that prints and exits and wrong for the one that takes the terminal over. Its logs now go to the instance log file, which `lazydap logs` already reads.

## [0.1.0] — 2026-07-31

The first release. A debugger you drive from the shell, one command at a time, with JSON as the contract. Rough edges are listed under [Known limitations](#known-limitations) rather than left for you to find.

### Added

**Debugging from the shell.** One binary, `lazydap`, with a subcommand per debug operation: `launch`, `continue`, `step` (aliased `next`), `step-in`, `step-out`, `pause`, `break`, `stack`, `scopes`, `variables`, `eval`, `threads`, `output`, `disconnect`. Paths are relative to your shell, so `lazydap break src/parser.c:142` means what it looks like it means.

**`--wait`, the flag that makes a debugger scriptable.** Every command that moves the program takes it. lazydap blocks until the program reaches a stable state (paused, exited, terminated) and returns one JSON object covering the whole trip: the stop reason, the top frame, which breakpoints were hit, breakpoint state that changed on the way, other threads that stopped, and every line the program printed. `--timeout N` bounds the wait (default 30s, `0` waits forever, `LAZYDAP_TIMEOUT` sets your own default). An adapter that dies mid-wait comes back as `"state": "adapter_died"` rather than a hang.

**JSON as the contract.** `--format json` for one object, `jsonl` for streams, `csv` for spreadsheets, `ids` for `xargs`, `table` for your eyes. The format is picked from the tty when you don't say: a pipe gets JSON, a terminal gets a table. Failures print a JSON object on stderr with a stable `error` name and leave stdout empty. Exit codes distinguish a failed command (`1`) from bad arguments (`2`), an unreachable daemon (`3`), and a missing adapter (`4`).

**Breakpoints that outlive the session.** `lazydap break file:line` records a breakpoint in `.lazydap/state.toml` under your project root and re-applies it to every later launch, whether or not a session is running when you set it. `--list`, `--remove`, `--toggle`, selection by `--id` or `--all`, and `--dry-run` on every mutation, using the same selection logic as the real thing. `--condition 'i == 7'` passes an expression to the adapter, so the program only stops when it holds.

**A daemon you never start.** The first command that needs one spawns it, per project root, and every later command in that directory finds it. It holds the debug session, keeps the adapter alive between your invocations, and buffers events so a command that arrives late still sees what happened. `lazydap status`, `lazydap shutdown`, and `lazydap daemon --foreground` when you want to watch it work.

**A terminal UI in the same binary.** Bare `lazydap` on a terminal opens it; in a pipe or a CI job that same command prints help. Three panes — source, call stack and variable scopes — with `Tab` between them, `<CR>` to jump to a frame or expand a variable, and `b` to set or clear a breakpoint on the cursor line. It drives the program with `F5`/`c`, `F10`/`n`, `F11` and `Shift-F11`, sending the same requests the CLI sends, and every one of its actions has a CLI equivalent because it is a client of the same socket. `j`/`k`/`<C-d>`/`<C-u>`/`gg`/`G` scroll. `q` leaves the TUI without ending the session, so you can carry on from the shell. If the daemon goes away underneath it, it says so and reconnects on its own — starting one if there is none.

**Launch configurations, including the ones your repository already has.** `lazydap launches list` shows every named way to start this project, from `.lazydap/state.toml` and from `.vscode/launch.json`, which lazydap reads and never writes. VS Code's dialect is understood as written — `//` and `/* */` comments, trailing commas — and `${workspaceFolder}`, `${workspaceFolderBasename}`, `${userHome}` and `${env:VAR}` are expanded. A variable nothing can expand is **left in the string and reported**, not quietly replaced with nothing, and the configuration is marked unrunnable rather than launched at a path that is missing a piece. Configurations for adapters lazydap does not ship are listed too, with the reason they cannot run. `lazydap launches run "Debug binary"` starts one, taking its program, arguments, working directory and environment from the file.

Both dialects are read where they differ: Microsoft's `cppdbg` spells the environment `environment: [{name, value}]` and its entry stop `stopAtEntry`, and both are honoured — a configuration that sets `LD_LIBRARY_PATH` is launched with it rather than silently without. `args` is accepted as a list or as one shell-style string, quotes and all; a string with an unterminated quote makes the configuration unrunnable with that as the reason, rather than being guessed at.

**A config file.** `~/.config/lazydap/config.toml` — or `$XDG_CONFIG_HOME/lazydap/config.toml`, or wherever `LAZYDAP_CONFIG_PATH` says; the first that exists wins, and the platform's own config directory (`~/Library/Application Support` on macOS) is searched last so a file written there still works. Two settings are read: `[adapter.codelldb] command` pins the adapter binary — the first tier of adapter discovery, ahead of `PATH`, and a pinned path that is not there is an error rather than a quiet fall-through to a different build — and `[general] wait_timeout_seconds` sets your own default for `--wait`, under `--timeout` and `LAZYDAP_TIMEOUT`. No file is needed; without one, lazydap runs on its defaults and writes nothing.

**A debuggee that dies with its debugger.** If the adapter is killed without stopping the program first, lazydap reaps the process it launched — after checking the pid still names that program, because a recycled pid belongs to a stranger.

**A live event stream for long-lived clients.** A client can subscribe to the kinds of event it cares about and be pushed frames as they happen, interleaved with replies to its own requests. It is answered with a state snapshot taken when the stream opens. This is what the TUI uses, and it is available to anything else that speaks the socket.

**An agent skill.** `lazydap.skill` at the repository root, packaging usage guidance, a full command reference, output schemas, error codes, and worked debugging sessions. The command reference is generated from lazydap's own argument parser and CI fails when the committed artifact drifts, so an agent never reads about a flag that no longer exists.

**`lazydap doctor`**, which reports whether codelldb is on `PATH`, where your project state lives, and which daemon is answering.

**Shell completions** for bash, zsh, fish, elvish and PowerShell via `lazydap completions <shell>`, generated from the same command tree as `--help`.

**Breakpoints bind under symlinked directories.** A program under `/tmp` on macOS never used to stop: `/tmp` is `/private/tmp`, lazydap canonicalises source paths, the compiler records what you typed, and codelldb compares the two as strings. When the adapter declines a path but names one it could have used, lazydap now re-sends that file's breakpoints under the name it offered — once, only when nothing in the file bound, and only when the suggestion resolves to the same file.

**Two codelldb behaviours normalised**, with the adapter's own answer preserved next to lazydap's. `--stop-on-entry` reports `reason: "entry"` and keeps codelldb's `"exception"` in `raw_reason`, because codelldb implements entry-stop with a `SIGSTOP` that LLDB classifies as an exception. `eval` defaults to the `watch` context, because `repl` makes codelldb run an LLDB *command* — under which `eval "x"` reads memory instead of reading your variable. `--context repl` is still there when you want the command interpreter.

### Known limitations

- **codelldb only.** C, C++ and Rust, or anything else LLDB can debug. debugpy, delve and js-debug are planned and not built.
- **One session per project at a time.** A second `launch` while one is live returns `SessionAlreadyActive`. Session ids are in the protocol so multi-session stays possible later.
- **macOS and Linux.** Windows is not a target.
- **The TUI has no REPL or watch pane**, and no mouse or theming. Setting a *conditional* breakpoint needs the CLI; `b` in the TUI sets a plain one.
- **The config file is smaller than the blueprint describes.** Two settings are read — the adapter's path and the `--wait` default. Themes, log rotation, socket directories and output defaults are documented in `docs/blueprint/08-state-and-config.md` and not implemented; unknown keys are ignored rather than rejected, so a config written against that schema keeps working as fields land.
- **Launch configurations are read, never written.** There is no `launches add`; lazydap's own go into `.lazydap/state.toml` by hand, and `.vscode/launch.json` belongs to VS Code. `attach` configurations are listed and cannot be run.
- **A `variables_reference` is invalidated whenever the program moves.** Re-run `scopes` after each stop.
- **No `attach`, watches, or restart.**
