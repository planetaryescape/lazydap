# M22 — Third adapter: delve (Go)

## What

Go debugging through Delve's DAP mode, behind the `DebugAdapter` trait (D052), reaching
parity with the codelldb/debugpy workflows: break, launch, `continue --wait`, stack, scopes,
eval, watches, TUI marker.

## Why

The competitive audit (docs/research/2026-07-31) found the nearest rival ships dlv and
js-debug; the user's call: "let's learn from debug-skill and create the adapters they have
that we don't." Delve first — it speaks DAP natively and its headless mode is the
best-documented of any debugger.

## How

1. `dlv dap --listen=127.0.0.1:0` serves DAP over TCP, one session per process —
   `Spawn::Tcp` like codelldb, but the port comes from dlv's stdout announcement (verify the
   exact line live before coding; do not trust this file). Discovery: config pin > PATH `dlv`
   + a liveness probe (`dlv version`).
2. Adapter selection: `.go` extension → delve compiles-and-debugs via its `debug` launch
   mode; a compiled Go binary stays codelldb-able but `--adapter delve` must win. Check what
   launch request shape dlv dap wants (`mode: debug|exec|test`) — support `debug` and `exec`
   minimum, `test` if cheap.
3. Entry stop, `process` event, exited/terminated ordering, reverse requests: spike first
   with a raw transcript (the debugpy spike pattern) and record every deviation as
   quirks/D-entries. Known from research: dlv dap is single-use per connection — the daemon
   must spawn per session, not reuse.
4. `examples/go-hello/` fixtures mirroring c-hello/py-hello; a serialized `wait_delve.rs`
   suite with the static-mutex + stable-fixture-path discipline.
5. launches import: `type: "go"` configs map to delve (runnable), and the not-runnable
   reason for Go configs is deleted.
6. Docs: adapter page updates (README scope line, why-lazydap when-to-use-else, install page
   gets a dlv prerequisite note), skill regeneration, quirks file `docs/reference/delve-quirks.md`.

## Success criteria

The full agent loop on a Go fixture, live: break, launch, continue --wait breakpoint blob
with captured stdout, eval, exit blob; watches evaluate across a stop; TUI marker lands;
AdapterNotFound (exit 4) with an install hint when dlv is missing; zero strays including Go
debuggees (D045 reap verified for dlv's process model — spike whether dlv kills the debuggee
on adapter death like debugpy or orphans it like codelldb).

## Depends on

- D052 trait (shipped, M18). Go toolchain + dlv on this machine (install user-local if
  absent; record versions).

---

## Completion notes (2026-07-31)

Done. Full agent loop live on Go: `break`, `launch --stop-on-entry`, `continue --wait` to a
breakpoint with captured stdout, `eval`, `stack`, `scopes`, exit blob with code 0. Zero
strays. `wait_delve.rs` is **11 tests**; `wait_codelldb.rs` (13) and `wait_debugpy.rs` (9)
stayed green throughout.

Verified against **delve 1.27.0** / **Go 1.26.5** / macOS arm64. `dlv` was not on this
machine; installed with `go install github.com/go-delve/delve/cmd/dlv@latest` into
`~/go/bin`, which is **not on `PATH` by default** — the single most likely reason `doctor`
reports delve missing on a machine that has it, now called out on the install page.

### What this file got wrong

Spiked before writing any code, per the M18 playbook. Four of its claims did not survive
contact:

1. **"the port comes from dlv's stdout announcement"** — right about stdout, and that was the
   part worth checking, because `spawn_tcp` only knew how to read **stderr**. It also
   hard-coded codelldb's `--port 0`, its `RUST_LOG=debug`, and its `Listening on ` marker.
   All four differ for delve, so `Spawn::Tcp` became a `TcpSpawn` recipe (D062) rather than
   growing a second spawn function.
2. **"dlv dap is single-use per connection — the daemon must spawn per session, not reuse"**
   — not reproducible. The listener accepted a second connection after the first session
   ended. It is moot either way, since lazydap spawns per session like codelldb, but the
   file stated it as a constraint and it is not one.
3. **"a liveness probe (`dlv version`)"** — implemented as `dlv help dap` instead. Every
   delve ever built prints a version; what has to be true is that this one has the DAP
   subcommand (added in 1.6). The failure without the probe is a launch that times out
   having never announced a port, which is exactly the mystery `Incomplete` exists to avoid.
4. **"spike whether dlv kills the debuggee on adapter death like debugpy or orphans it like
   codelldb"** — it does **both**, depending on state. Paused at a breakpoint: the debuggee
   dies with the adapter. Running: it survives and is reparented to init. The task file
   assumed one answer; there are two.

### The thing the task file could not have known

`outputMode: "remote"` is not optional. delve's default writes the debuggee's stdout to
**the adapter's own stdout**, which lazydap does not read — so every `continue --wait`
returns `captured_output: []` while the program is visibly printing, with no error anywhere.
This would have shipped as "Go support works except output", which is most of what the wait
blob is for.

### A real bug, found by the suite

The D045 reaper declined to kill its own debuggee. It identifies one by matching the `ps`
command line against the path that was launched; delve's `mode: "debug"` compiles the `.go`
source and runs a *different* file, so the check concluded "somebody else" and left it
running. Found by the adapter-kill test leaking two Go debuggees onto this machine — the
same way D045 itself was found.

Fixed as **D061**: take the `process` event's `name` — what the adapter says it actually ran
— and fall back to the launched path when it gives none. One rule for all three adapters.
Only used when it is an absolute path, so an adapter that puts a label there cannot aim the
reaper at whatever matches.

### Deviations from the plan

- **`mode: "test"` not implemented.** The file said "test if cheap". It is not: the filename
  cannot carry the distinction, since `foo_test.go` is an ordinary program to `debug`, so it
  needs a CLI surface that does not exist. `debug` and `exec` both ship, which was the
  stated minimum.
- **`output` launch argument added**, which the file did not mention. delve compiles to
  `__debug_bin<random>` in the *adapter's* working directory — the daemon's, so a user's
  repository. It cleans up on `disconnect`, but not on a hard death. Pointing it at a
  temporary path keeps the leak out of working trees and gives leaked Go debuggees a
  greppable `lazydap-delve-<daemon pid>-` name, which is what the suite's stray check
  uses — scoped to its own sandbox's daemons, because `pgrep` is machine-wide.
- **The entry stop has no stack.** delve stops before the Go runtime has scheduled anything,
  so `threads` returns a placeholder named `Dummy` and `stackTrace` fails outright. Left as
  an honest error rather than papered over as an empty stack; asserted in the suite and
  written up as quirk 6.
- **An unrecovered panic pauses** (`reason: "exception"`) where debugpy's uncaught exception
  exits. delve applies its own `unrecovered-panic` default server-side despite lazydap
  sending no exception filters. Asserted, because an agent that learned the Python behaviour
  will be wrong here.

### Follow-ups discovered

- `mode: "test"` wants its own milestone — it is the feature Go users will ask for first.
- The debuggee reaper still cannot identify a program whose path contains a space; unchanged
  by this milestone, noted again because a third adapter did not fix it.
- `wait_codelldb.rs` and `wait_debugpy.rs` assert strays in `Drop` without checking
  `std::thread::panicking()`, so a failing test there aborts the process and loses its own
  message — which is what happened here before `wait_delve.rs` guarded against it. Not
  changed in those files (blast radius), but they have the same latent hazard.

## Review fixes (2026-07-31)

Four issues found in adversarial review, fixed before merge:

1. **Protocol bump to v6 (D063).** Adding the `Delve` variant changed the wire without a new
   request or field, so a v5 daemon passed the version handshake and then dropped the
   connection on the first Go launch instead of triggering the `VersionMismatch`
   auto-replacement. Bumped `LAZYDAP_PROTOCOL_VERSION` to 6; the frozen `Shutdown` escape
   hatch and its cross-version test are version-agnostic and hold.
2. **`mode` read from `launch.json`.** The adapter guessed `debug` from the `.go` extension
   alone, so a `mode: "debug"` on a package *directory* went out as `exec` and delve rejected
   it, and a deferred `mode: "test"` config was marked runnable. Now: the adapter infers a
   directory as `debug`; the import maps debug/exec/test explicitly, blocks `test` as deferred
   (`NotRunnable::DelveMode`), and blocks `exec` naming a `.go` source. Five new import tests
   plus a package-directory adapter test.
3. **Bounded the port-announcement wait.** `spawn_tcp` waited forever for the marker; the
   30s launch deadline only starts later at the handshake, so an adapter that lived but never
   announced hung the client and held the session slot, bricking future launches. Added a
   15s spawn deadline (injectable for the test) that kills the child and returns an honest
   error; the reservation frees on that error like any launch failure. Applies to codelldb
   too (shared path). Tested with a `sleep` process that never announces.
4. **Track and delete the `mode: debug` temp binary.** delve deletes it on a clean
   `disconnect`, but an adapter that died, or a session that *exited* and was then shut down,
   left it behind. `Session::clean_compiled_artifact` (guarded by the `lazydap-delve-` prefix
   + temp-dir, so a user's `exec` binary is never touched) is now called on all four teardown
   paths: adapter death, disconnect, shutdown drain, and finished-slot reap. The
   exited-then-shutdown gap was the one the live run caught. `wait_delve`'s stray check gained
   a baseline-scoped file sweep, and a dedicated shutdown-cleanup regression test.

Also fixed the `Drop`-without-`panicking()`-guard in `wait_codelldb` and `wait_debugpy`
(their `assert_no_orphans` aborted on a double panic and ate the real failure's message —
the hazard that bit `wait_delve` during M22).

Gates at fix time: fmt/clippy clean, boundaries ok (7 crates), wait suites
**codelldb 13 / debugpy 9 / delve 12**, daemon lib 209, config 75, dap 21, protocol 21.
Go live loop re-run green; zero strays including temp binaries.
