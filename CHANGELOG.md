# Changelog

All notable changes to lazydap are recorded here.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The **lazydap protocol** is versioned separately from the binary. It is at **v9**; a daemon left running from an older build refuses connections with `VersionMismatch`, and `lazydap shutdown` clears it — which the TUI now does for itself.

## [Unreleased]

### Fixed

**One enormous event dropped a subscriber's whole connection.** The daemon refuses to frame anything over 16 MiB, and a reply that big has been answered with an error rather than a hang-up since v0.2.2 — but an event pushed to a subscriber has no request to refuse, so the connection was closed instead. A TUI watching a program that printed one gigantic line lost its event stream and reconnected. The event is now dropped on its own, with a `warn!` in the daemon log naming the kind and the size, and the connection keeps serving (D091, amended).

**Anything lazydap wrote to stderr panicked when the pipe reading it closed.** `println!` and `eprintln!` both panic on `EPIPE`, and while results have gone through a printer that treats a closed reader as a clean end since v0.2.3, every error, warning and usage message still used `eprintln!` — as did `clap_complete`, which writes the completion script to stdout itself. So `lazydap completions bash | head`, `lazydap --format json nosuch 2>&1 | true` and `lazydap break /nope.c:1 2>&1 | true` all exited 101 with `Broken pipe` where the shell expected 0, 2 and 1. Every writer now goes through the same rule.

**Two spellings of one file made two breakpoints, and a hand-edited one could not be removed at all.** The store compares source paths for equality, and neither end resolved them: on macOS, where `/tmp/p/main.c` and `/private/tmp/p/main.c` are the same file, a client that did not canonicalise for itself got a second breakpoint on a line that already had one — and a breakpoint typed into `.lazydap/state.toml` by hand under either spelling could not be selected by location, so `break --remove main.c:1` reported `removed`, listed nothing as missing, and removed nothing. The daemon now resolves what arrives and the store resolves what it loads, so both ends agree (D100). `lazydap break` and the TUI resolved their own paths already and were unaffected. A path that will not resolve — a file not generated or checked out yet — is still kept exactly as written.

### Changed

**`install.sh` uses `GITHUB_TOKEN` when there is one.** Its release lookup goes to GitHub's anonymous API, which allows 60 requests an hour per address; a few installs in a row and it answers 403 for the rest of the hour. `GITHUB_TOKEN` or `GH_TOKEN`, if set, now authenticates that one request. It is sent to `api.github.com` and nowhere else — never to the asset download, which is a different host and needs no credentials — passed to curl through a config file on stdin so it stays out of the process list, and never printed. A token GitHub rejects is not fatal: the lookup is retried anonymously, which is what an expired token left in a shell profile needs it to do.

**The daemon no longer implements the `doctor` checks it stopped being asked for.** `doctor`'s adapter and state checks moved into the CLI in v0.2.3, because both describe the machine and directory you typed the command in rather than the daemon's. The daemon-side versions stayed behind, reachable only from a test — two implementations of one answer, with nothing saying which was live. The request's `check_adapters` and `check_state` fields are still decoded, and ignored; they come off the wire at the next protocol bump (D093, amended).

## [0.2.7] — 2026-08-18

### Changed

**The docs describe the debugger that ships.** The skill, README, AGENTS.md, the site and the install text still described the v0.1.0 world in places: one adapter, a `DapProtocolError` for a reused handle (it is `StaleHandle`), an untagged v0.1.0, no deploy for the docs site, a doctor that failed on any missing adapter. All of it now says what the code does — three adapters and how one is picked, the `--wait` blob's `locals` and `user_frame`, `break` editing a location in place, doctor's verdict rule, the campaign's decisions D084–D099. The site's CLI reference generator also reads clap's singular `[alias: c]`, which newer clap prints for a single alias.

## [0.2.6] — 2026-08-18

### Changed

**CI runs the debugger against real adapters.** codelldb, debugpy and dlv are installed in the pipeline and the suites that drive them fail rather than skip when one is missing, on every pull request and again on the tagged commit a release is built from. They had been skipping themselves in CI since the day they were written, so "tested against three real adapters" rested on a maintainer running them by hand.

### Fixed

**A breakpoint update could arrive with no id to match it against.** codelldb answers `setBreakpoints` and then, microseconds later, sends a `breakpoint` event about the same breakpoint. lazydap recorded which adapter breakpoint id belonged to which of its own *after* that answer came back to the caller — by which time the daemon's read pump had already dispatched the event — so the update went out with `id: null`, and `--wait`'s `breakpoint_updates` named a breakpoint no client could find in `break --list`. The mapping is now recorded as the answer goes past the pump, before anything that followed it on the wire is dispatched. On macOS codelldb happens to send a second event 20 ms later, which papered over this; on Linux it does not.

## [0.2.5] — 2026-08-18

### Fixed

**The TUI drew no breakpoint sign for a file reached through a symlink, and `b` there added a second one.** `lazydap break` canonicalises before it records a breakpoint, so on macOS — where `/tmp` is `/private/tmp`, and a checkout under a symlinked directory is ordinary — `lazydap break /tmp/d/hello.c:6` stored `/private/tmp/d/hello.c`. The program stopped there, the TUI opened the file under the adapter's spelling, and line 6 showed nothing; pressing `b` on it recorded a duplicate under a name `lazydap break --remove /tmp/d/hello.c:6` could not select. The pane now holds the file under the name the filesystem gives it, which is the one every breakpoint is recorded under (D097).

**A daemon that died as fast as the TUI could start one was restarted four times a second.** The reconnection ladder counts from 250ms and doubles to 4s, and a connection coming back put it straight back at the bottom — so a daemon that accepted the connection and then died on the first request it was given (a crash on `Subscribe`, or something killing it in a loop) had the TUI spawning another one every quarter second for as long as it was open. The ladder now keeps its place until a connection has lasted five seconds, and still never gives up (D096).

**A panic in the TUI left the terminal wrapping everything you pasted.** ratatui's panic hook restores raw mode and the alternate screen; it knows nothing about bracketed paste, which the TUI turns on itself, so a crash left the mode set and every later paste in that shell arrived surrounded by `\x1b[200~ … \x1b[201~` until the user ran `reset`. The hook is now wrapped with one that turns it back off.

**Pressing Enter in the add-watch prompt while the daemon was away threw the expression away.** The prompt was taken out of the state before the key was looked at, and the refusal never put it back: the notice said the daemon was unreachable and the text was gone, with nothing on screen to retype it from. It stays open, with what was typed still in it.

**Expanding a very large container made the scopes pane redraw slowly.** Every read of the pane's rows walked the whole visible tree and allocated a row and a string per node, and one draw did that three times, ten times a second — so a 100,000-element array, which the pane deliberately does not truncate, cost millions of allocations a second. The rows are built when the tree changes and not when it is read.

## [0.2.4] — 2026-08-18

### Fixed

**One adapter process leaked per session that ended on its own.** codelldb, debugpy and delve all hold their DAP socket open after they report the program terminated, waiting to be disconnected from — and the daemon, which only ever read from that socket, kept the adapter alive with it. Three `launch` + `continue --wait` cycles left three codelldb processes running, each with its own copy of LLDB, until `lazydap shutdown`. The daemon now disconnects an adapter as soon as its session ends and pulls the plug if it will not go, so an agent can loop launch-and-continue without a closing `lazydap disconnect`.

**`lazydap disconnect --no-terminate` killed the program it promised to keep, and said it had not.** Against codelldb the daemon killed the adapter before it had finished detaching, then read that killed adapter as one that had crashed — which is the case that reaps an orphaned debuggee (D045) — so the program died and the response said `terminated_debuggee: false`. The other two adapters cannot detach at all: delve's debuggee is its own child and dies with it, and debugpy simply never answers a disconnect that asks it to detach, so the command sat for twelve seconds and then killed the program anyway. Both still reported `false`.

Only codelldb advertises DAP's `supportTerminateDebuggee`, and only codelldb honours it. `--no-terminate` is now honoured where it can be — the debuggee is released before anything that could look like a crash, and the adapter is given time to leave — and carried out as an ordinary terminate where it cannot, in 0.06 s rather than 12, with `terminated_debuggee: true` saying what happened and a one-line warning on stderr saying why. `--dry-run` makes the same decision, so the preview cannot promise what the mutation will not do.

**Adapter output that is not UTF-8 no longer wedges the adapter.** One such byte on a log stream ended the loop that drains it; the pipe then filled, and an adapter blocked writing a log line answers no requests. A `Content-Length` larger than 256 MiB is also refused now rather than allocated on the adapter's say-so, and a failed launch reaps a debuggee the adapter had already started instead of orphaning it.

## [0.2.3] — 2026-08-18

### Changed

**`lazydap doctor` passes when at least one adapter is usable.** It exited `1` if *any* adapter was missing, so a Mac with codelldb and no Go toolchain failed the check that the README, `install.sh` and the Homebrew formula all end with. A missing adapter is now reported per adapter as `missing`, with where to get it, and only fails the run when it was the last one. Everything else — the config file, the state file, the daemon — still has to pass (`D093`).

**`lazydap doctor` reports the adapters your shell would use.** Discovery now runs in the process you typed the command in, against its config and its `PATH`, the same way a launch resolves one (D050). The daemon answered from whatever environment it inherited whenever it started, which may have been days ago in another directory.

### Fixed

**`lazydap launch ./app --cwd sub` looked for the program inside `sub`.** `--cwd` says where the *debuggee* should run; the program on the command line is the one your shell can see. Resolving it against `--cwd` failed with `cannot debug ./app: No such file` for a binary plainly there — and when `sub/app` happened to exist too, debugged that one instead without saying so.

**`lazydap logs --follow` mixed raw log lines into `--format json`.** It printed the JSON object, then appended bare lines under it, which no parser survives. `--follow` now takes `--format table` or `--format jsonl` — under `jsonl` each line arrives as the same `{"line": ...}` object `lazydap logs` already prints — and refuses any other format with a usage error before printing anything.

**Piping lazydap into `head` crashed it.** `lazydap break --list --format jsonl | head -1` exited 101 with a `Broken pipe` panic across stderr. A reader closing the pipe is how `head` works, not a failure: every write to stdout now ends the command quietly with exit 0.

**One usage error called itself something else.** `--format ids` on a result that is not a list reported `"error": "BadRequest"` while every other usage mistake reported `"error": "UsageError"`, so a script had to know both. Usage messages also no longer carry a `BadRequest:` prefix under a `UsageError` label — one name per mistake.

**`--format table` is honoured for the errors clap raises.** `lazydap --format table nosuch | cat` answered a request for prose with JSON on stderr, because the format was guessed from the pipe even though the caller had said. An explicit format now wins in both directions.

**A `LAZYDAP_TIMEOUT` nothing can read is reported.** `LAZYDAP_TIMEOUT=5m` was silently ignored and every `--wait` in that shell quietly used 30 seconds. It is now a usage error, refused before a daemon is started. An explicit `--timeout` still wins, so the variable is only consulted when it is going to be used.

**A broken `.vscode/launch.json` no longer hides the project's own launch configurations.** VS Code owns that file, and a stray comma in it failed `launches list` and `launches run` outright — including for configurations that came from `.lazydap/state.toml` and had nothing to do with it. The file-level failure is now a warning next to the list, the same way an unreadable single configuration inside it already was.

**An unterminated `${` in a launch configuration is reported instead of run.** `"program": "${workspaceFolder/app"` was passed through verbatim and listed as runnable, so the launch failed on a path with a `${` in the middle of it. It now joins the other variables nothing could expand, and `launches list` shows the typo as the reason.

**`lazydap doctor --check-state` no longer needs a daemon.** It reads `.lazydap/state.toml` itself and reports a parse error with its line and column — which matters because a state file the daemon refuses to start on is exactly the case the check exists for. A plain `lazydap doctor` also reports a daemon that will not start as a failed check rather than aborting, so one command names the reason.

**Every `PathsError` said `InvalidProjectRoot`.** A socket path over the length limit, a runtime directory owned by somebody else, a missing home directory — all of them are about the directories lazydap keeps its own socket, lock, pid and log in, and none is about the project root. They now report `DaemonUnreachable` and exit `3`, which is the retryable code a script should see.

## [0.2.2] — 2026-08-18

### Fixed

**`--wait` could report `timeout` for a program that had already stopped.** A debuggee chatty enough to outrun the wait's own event stream lost whatever was in the dropped range — and if that held the `stopped`, the wait sat there until its deadline while `lazydap status` said `paused`, which is the worst kind of wrong answer because nothing in the blob says it is one. Falling behind is now reconciled against the session's own record of what happened, so the outcome is the truth even when the events that carried it are gone. The arithmetic that made a wait slow enough to fall behind is gone too: the output cap re-summed every chunk it had kept, for every chunk that arrived.

**A `continue --wait` on an already-running program could miss the stop it was waiting for.** Nothing is sent for one of those — the program is already doing it (D055) — and a stop reached in the gap between deciding that and subscribing belonged to nobody: the subscription was too late and the backlog deliberately does not adopt stops. It now reports that stop, and only that stop; the one the program was already sitting at still belongs to the run before.

**A `pause --wait` racing a `continue --wait` from another client could report a crash.** The already-running `continue` sent nothing but still cleared both in-flight markers, so the `SIGSTOP` codelldb was about to deliver arrived with nothing to explain it and came back as `"reason": "exception"` — the bug fixed for the ordinary case in v0.1.0, from a path that installs a marker for a request it never makes.

**A program that finished while another client's request was queued left the session wedged.** The finished check happens before the request queues for the session's execution permit, and a `continue --wait --timeout 0` from somebody else can run the program to its exit in between. The queued request then stamped the session back to `running` and asked a dead adapter to step: nothing put it back, nothing reaped it, and every later `lazydap launch` was refused with `SessionAlreadyActive` until somebody ran `disconnect`. It is now refused with the same error a step on a finished session has always given.

**Hanging up on a `--wait` no longer wedges the session for everyone else.** A `continue --wait --timeout 0` holds the session's execution queue for as long as it runs, so a Ctrl-C left every later `continue` and `step` — from any client — waiting behind a caller that was not there, until each hit its own deadline and reported the daemon as unreachable. The daemon now notices the connection closing and ends that wait, and nothing else: a request already talking to the debugger runs to completion.

**A reply too big for one frame is now an error rather than a closed connection.** The socket carries frames up to 16 MiB, and a reply past that — `variables --max 0` on a very large container, `output` on a session that printed a great deal — could not be encoded, so the daemon hung up and the client reported "the daemon closed the connection before answering" with exit 3: an unreachable daemon, for a request it had understood perfectly. It now answers `BadRequest` saying what happened and which flags narrow the question, and the connection stays usable.

## [0.2.1] — 2026-08-18

### Changed

**Protocol v8 → v9.** `action` on a breakpoint report gained `updated` and `unchanged`, so that setting a location that already has a breakpoint can say which of the three things it did. A new variant on an enum that crosses the wire is not additive — a v8 client fails to decode the whole frame rather than the one field — so it is a bump, the same way a new `AdapterKind` (v6) and a new `ErrorCode` (v8) were.

**Setting a breakpoint on a location that already has one now edits it.** `lazydap break x.c:10 --condition 'i > 5'` on a line you had already broken on updates that breakpoint in place, keeping its id, and reports `"action": "updated"` — or `"unchanged"` when you asked for exactly what was already there. The whole request wins, including the parts you left out: no `--condition` means no condition, the same as it does on the first call — and that covers `enabled` too, so a bare re-set re-enables a breakpoint you had disabled with `--toggle`. Pass `--disabled` to keep it off.

### Fixed

**`lazydap break FILE:LINE --condition ...` silently dropped every modifier when the line already had a breakpoint.** The command answered `"action": "added"` with `enabled: true` and no condition, exit 0, and the debugger went on using the old unconditional breakpoint — so a script that narrowed a breakpoint it had set earlier debugged against something else entirely. Setting a location now edits what is there and reports `updated` or `unchanged`, and `--dry-run` previews the same decision.

**A breakpoint the adapter refused was recorded without telling anybody.** The store was changed and the change announced only after the adapter had accepted it, so a `setBreakpoints` that failed — usually an adapter that had just died — left the caller with an error, the project with the change, and a TUI drawing the list from before it. The announcement now goes out before the adapter is told, and the error says the change is recorded and will apply at the next launch, naming the ids.

**Two clients removing or toggling the same breakpoint could both report success.** `not_found` was worked out from a selection taken before the lock the mutation ran under, so the loser of the race answered with no breakpoints, no missing ids and exit 0 — success, for work it did not do. Selection and mutation now happen under one lock, as they already did for watches.

## [0.2.0] — 2026-08-18

### Added

**Watch expressions.** `lazydap watch add <expression>`, `watch list` and `watch remove`, with `--dry-run` on both mutations and `--format ids` for piping. A watch is project state, recorded in `.lazydap/state.toml` beside your breakpoints: you can set one before anything is running, and it is still there after the daemon has gone. What it *evaluates to* is not recorded, because that is only true while the program is sitting still.

**A watches pane in the TUI.** `Tab` reaches it, `a` adds an expression, `dd` removes the selected one. Every expression is re-evaluated each time the program stops, and again when you select a different stack frame — so the watches and the scopes pane are always talking about the same function. An expression that is out of scope keeps its row and shows the adapter's complaint, because the same expression is usually back in scope a few steps later.

**A REPL pane in the TUI.** `Tab` reaches it, type an expression and press `<CR>`; the answer appears under the line that asked for it. It sends the same request `lazydap eval` sends, in the same `watch` context and for the same reason — `repl` context makes codelldb run an LLDB *command*, so `x` becomes a memory read rather than your variable. Adapter commands are still one keystroke away: a line starting with `/` goes to the adapter, so `/bt` is a backtrace. `<C-p>` and `<C-n>` walk the history, which lasts for the session.

### Changed

**Protocol v4 → v5.** The watch requests and the `WatchUpdated` event. A `Request` variant an older daemon does not know is not a soft failure — it cannot decode the frame at all, so it never reaches the version field it would have refused on. The bump turns "old daemon still running" into the `VersionMismatch` that `lazydap shutdown` clears and auto-spawn replaces.

**A stop now answers the next two questions as well.** The `--wait` blob carries `user_frame` — the nearest frame in *your* code, for when the program died inside a library and the frame it stopped in is `_platform_strcmp$VARIANT$Base` with no file you can open — and `locals`, the variables of whichever of those two frames is the one worth reading. `frame` is untouched and still says exactly where it stopped. Reading a local was two commands and diagnosing a crash was five; both are now one. Measured at about a millisecond against a wait that is dominated by the program itself, so it is always on.

**`lazydap variables` has a default cap.** 200 rows, with `truncated` on the response saying when it bit, `--start` to page and `--max N` (or `--max 0`) to raise or lift it. A `Vec` of two thousand used to come back as two thousand and one rows with nothing to indicate it. Values themselves are never shortened — a truncated list is recoverable, a truncated value is a claim about the data.

**`0` means "no limit" on `--levels`, `--count` and `--max`,** the way it already did on `--timeout`. `stack --levels 0` used to return an empty list of frames under exit 0.

**Protocol v5 → v7.** v6 added the `delve` adapter — a new `AdapterKind` variant, which an older daemon cannot decode at all. v7 changed four things about what the daemon reports: `threads` may omit a thread's `name`, the `--wait` blob and the `Stopped` event gained `adapter_thread_id`, `capabilities` gained `supports_variable_paging`, and a variable gained `evaluate_name`.

**Protocol v7 → v8.** `frame_id` and `variables_reference` are lazydap's own handles rather than the adapter's numbers, `Response::Continued` gained `already_running`, `Response::Variables` became a struct with `truncated`, the `--wait` blob gained `user_frame` and `locals`, and `ErrorCode` gained `StaleHandle`.

**`lazydap variables --start` and `--count` now work against every adapter.** codelldb does not implement DAP's variable paging and silently ignored both, so `--start 100 --count 5` on a 2000-element array returned all of it from `[0]`. lazydap applies the window itself when the adapter has not claimed it. `--filter` is passed through to the debugger and *not* emulated: nothing on the wire says which children are indexed, and guessing from how a name is spelled would return the wrong rows against an adapter that spells them differently.

### Fixed

**A stale `variables_reference` could return another *session's* data.** Handles were numbered per session, so one minted in a session that has since ended was a live handle in the next one — and because inspection commands resolve against whichever session is current, a reference remembered across a `disconnect` came back full of a different program's variables under exit 0. Handles are now numbered by the daemon and never reused, and one from an ended session is refused with `StaleHandle` saying so.

**`truncated` now means "there is more than you are seeing", whatever narrowed the list.** `--count 5` on a 2000-element container used to return five rows and `truncated: false`, which contradicts the field and stops a client that pages on it. When both `--count` and `--max` are given the narrower wins, and a window that reaches the end of the list correctly reports `false`.

**A stale `variables_reference` could return another frame's data.** The adapter's handles stop being valid the moment the program moves, and an adapter is free to hand the same number out again at the next stop for something else — so a reference remembered across a `continue` either errored obscurely or, worse, was answered with somebody else's variables under exit 0. lazydap now mints its own handles, one per stop and never reused, and refuses one from an earlier stop with `StaleHandle` before the debugger is asked anything.

**`eval --frame 0` claimed the program was running while it was stopped.** `--frame` takes an opaque frame id and `0` is the obvious thing to type; codelldb reports an unresolvable frame id as *"can't evaluate expressions when the process is running"*, which is false and sends an agent off to poll a program that is never going to move. Unknown frame ids no longer reach the debugger: the refusal names the problem and says that ids come from `lazydap stack`. The `--help` for every `--frame` says so too.

**`continue` on an already-running program reported a resume that never happened.** It answered `{"state":"running","thread_id":0}` under exit 0 — but nothing was sent, because there was nothing to resume, and `0` is what codelldb answers a thread query on a running process with rather than a real thread. It now reports `already_running: true` and no thread at all.

**`launch` returned breakpoints that contradicted themselves.** codelldb answers with `verified: true` alongside `Resolved locations: 0`, then corrects itself by event a moment later — making a working breakpoint look broken. A `message` is now kept only on a breakpoint that did *not* verify, where it is the reason.

**`pause --wait` on a program that was already stopped re-reported the previous stop** with a fresh `elapsed_ms`, indistinguishable from a pause that had worked. It is now refused, naming the state.

**`lazydap pause --wait` reported a crash.** codelldb implements `pause` by signalling the process, and LLDB calls a signal stop an exception — so asking a program to stop came back as `"reason": "exception"`, which reads as a segfault. It is now `"reason": "pause"`, with the adapter's own word kept in `raw_reason`. The same fix covers a `pause` racing a step: both are tracked, so neither answer is lost.

**`lazydap threads` invented a thread name.** Asked while the program was running, codelldb replies with one nameless thread `0` — a placeholder. lazydap filled that in as `"thread 0"`, which reads like a real answer about a real thread. `name` is now absent when the debugger gave none.

**`lazydap step --thread` reported a different thread.** codelldb answers a step aimed at one thread by naming whichever thread it had selected before — the one that did *not* move. That thread was reported, and became the default for the next `lazydap stack`. The blob now names the thread that was asked to step, with the debugger's answer kept in `adapter_thread_id`.

**`output_truncated` meant two different things and admitted to neither.** A run that outran the output cap kept accepting later output that still fit, so `captured_output` was a *splice* — hundreds of lines missing from the middle with the tail glued onto the cut. Separately, a program chatty enough to overrun the session's event buffer between two commands lost the *beginning* of its output and the flag stayed `false`. Both now set it, `dropped_events` says how many events were lost, and what you keep is a genuine prefix or a flagged suffix.

**`lazydap eval` returned errors as values with exit 0.** codelldb answers an expression it could not evaluate with a *successful* response whose result is the error text. Those now fail properly. The check is deliberately narrow — the literal `<error:` prefix — because `<last operation failed>` is a summary string a real program can have, and failing a legitimate value is worse than the bug. An unreadable address still comes back as `<read memory ... failed ...>` with exit 0; that gap is documented rather than papered over.

**Variables carry `evaluate_name`.** The debugger's own answer to "what expression names this row", and the only reliable way to turn a row called `[100]` into something `lazydap eval` accepts.

**A Python frame's `source.name` is no longer blank.** debugpy sends only `path` where codelldb and delve send both, so `frame.source.name` was missing for one language out of three. It is filled from the path lazydap already had.

**The TUI no longer writes its own log lines across its panes.** Every other command logs to stderr, which is right for one that prints and exits and wrong for the one that takes the terminal over. Its logs now go to the instance log file, which `lazydap logs` already reads.

**A typo in `.lazydap/state.toml` bricked every command for ten seconds each.** The daemon bound its socket and wrote its pid file *before* reading the project state, so a hand-edited file it could not parse left a socket nobody answers on — and every later command waited out the full spawn deadline before reporting `DaemonUnreachable` with the connection refusal, never the TOML error. The state file is now read first, so a daemon that cannot start leaves nothing behind, and the client watches the daemon it started: an immediate exit is reported straight away, with the daemon's own complaint in the message. A client that loses the spawn race no longer waits either — once the winner lets the lock go without a daemon appearing, it takes its own turn and gets the same answer. Ten seconds and a misleading error became well under one and the parse error, with the line and column in it, for every command racing at once.

**The state file's durability and its hand edits both got stricter.** The temporary file is now `fsync`ed before the rename, so a power cut cannot leave a `state.toml` that is present and empty; abandoned `state.toml.tmp.<pid>` files from a crash mid-write are swept on the next write. External edits are noticed by comparing bytes rather than mtimes, so an edit landing in the same clock tick as lazydap's own write is no longer silently reverted — and a breakpoint or watch *deleted* by hand now stays deleted instead of being written back on the next flush. A file that is missing or empty is not read as a deletion of everything, so `rm -rf .lazydap` or an editor caught mid-save cannot cost you the project's state.

**A `.git` in your home directory no longer makes every directory one project.** The project-root walk had no ceiling, so with dotfiles in `~/.git` — or a stray `~/Cargo.toml` — any unmarked directory under `$HOME` resolved to `$HOME`: one `~/.lazydap/state.toml` and one daemon shared across everything you debug. The walk now stops at the home directory, which is only a root if you asked for it with a `.lazydap/` directory. A *file* named `.lazydap` no longer counts as the marker either; it has to be a directory.

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
