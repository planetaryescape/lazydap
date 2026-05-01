# 05 — Sessions

A session is one active debug context: one adapter process, one debuggee, one set of threads/frames/scopes.

## Lifecycle

```
   ┌────────────┐
   │   Idle     │ ◄── daemon running, no session
   └─────┬──────┘
         │ Launch / Attach
         ▼
   ┌────────────┐
   │Initializing│ ── DAP handshake in progress
   └─────┬──────┘
         │ initialize response received
         ▼
   ┌────────────┐
   │Configuring │ ── set breakpoints, configurationDone
   └─────┬──────┘
         │ configurationDone response
         ▼
   ┌────────────┐
   │  Running   │◄────────┐
   └─────┬──────┘         │
         │ stopped event  │ continue / step
         ▼                │
   ┌────────────┐         │
   │  Paused    │─────────┘
   └─────┬──────┘
         │ exited / terminated / disconnect
         ▼
   ┌────────────┐
   │  Ended     │ ── awaiting cleanup
   └─────┬──────┘
         │ adapter process reaped
         ▼
   ┌────────────┐
   │   Idle     │
   └────────────┘
```

Failure transitions:

- Any state → `AdapterDied` if adapter process exits unexpectedly.
- `Initializing` / `Configuring` → `Ended` if handshake fails.
- `Running` → `Ended` on `terminated` event.

## Single-session in v0.1

The daemon enforces N=1 in v0.1. Why and how:

**Why N=1:** Multi-session adds three real complications none of which we want to debug while building everything else:

1. **Which session does `step` apply to?** Has to default-route or require `--session <id>` everywhere. Both have UX costs we'd rather pay later, with experience.
2. **Adapter resource exhaustion.** Each codelldb takes ~50–80MB. Spawning many sessions silently is bad UX.
3. **Multi-session UI design.** TUI needs a session picker, a "current session" indicator, a way to switch contexts. That's a chunk of design work.

**How we enforce it without painting into a corner:**

The protocol is multi-session-ready from M5. Every session-scoped request carries `session_id`. The daemon implementation simply rejects `Launch` if a session already exists:

```rust
if !self.sessions.is_empty() {
    return Err(LazydapError::SessionAlreadyActive { existing: self.sessions[0].id });
}
```

Lifting the restriction post-v0.1 is a daemon-only change. Clients keep working.

**TUI / CLI behaviour in v0.1:**

- `--session <id>` flag exists on every command but is hidden in help and ignored if there's only one session.
- `lazydap status` reports `sessions: [{id, ...}]` (always 0 or 1 entries).
- Bare `lazydap status` defaults to the only session.

## Multi-session future (post-v0.1)

When the constraint lifts:

```
Daemon
├── HashMap<SessionId, Session>
├── default_session: Option<SessionId>     // most recently created
├── focus_session: Option<SessionId>       // user-chosen for "default" semantics
└── ...
```

Default routing rule: if `--session` not given, use `focus_session`, else `default_session`, else error "ambiguous, multiple sessions active".

CLI gets `lazydap session` subcommand:

```
lazydap session list
lazydap session focus <id>
lazydap session kill <id>
```

TUI gets a session picker (probably a top bar with active sessions, like browser tabs). `<C-s>` cycles focus.

Multi-session test scenarios:

- Two C binaries debugged simultaneously
- Mixed-language: C client + Python server
- Same binary, multiple instances (load test)
- One adapter dies: other survives

## Session ownership and cleanup

The daemon owns sessions. When does a session die?

| Trigger | Behaviour |
|---|---|
| `lazydap disconnect` (client-initiated) | Send DAP `disconnect`, wait 1s for response, kill adapter process, emit `SessionEnded { reason: UserDisconnect }`, remove from session map |
| `lazydap disconnect --terminate` | Same but `disconnect.terminateDebuggee = true` (kills program) |
| Program exits (`exited` event) | Clean shutdown of adapter, emit `SessionEnded { reason: ProgramExited }`, persist any state changes |
| Adapter dies (SIGCHLD) | Synthetic `terminated` event, emit `SessionEnded { reason: AdapterCrashed }`, clean up daemon-side state |
| Daemon shutdown | All sessions get `disconnect --terminate`, emit `SessionEnded { reason: DaemonShutdown }` |
| Idle timeout (config) | If session paused >N minutes with no client subscribed, prompt to disconnect (via log warning, not blocking) |

## Reverse requests

DAP allows the adapter to send "reverse requests" to the client — the most common is `runInTerminal` (asking the client to spawn a terminal for the debuggee's stdio).

lazydap policy:

- Default: refuse `runInTerminal`. The daemon responds with `body: { kind: "console" }` redirecting I/O to the integrated console (DAP `output` events).
- Configurable: per-launch, set `terminal: "external"` in `LaunchConfig` to spawn an actual terminal. Implementation: daemon spawns the user's `$TERMINAL` (or `xterm`/`Terminal.app` per-platform) running the debuggee.
- Future: route reverse requests as `Reverse` events to subscribed clients, who can opt to handle them.

## Session state queries

Three ways clients learn session state:

1. **Snapshot:** `lazydap status --format json` returns the current session struct. One-shot.
2. **`Subscribe { channels: [Stopped, Continued, ...] }`:** stream changes as they happen. The TUI uses this.
3. **`GetStateSnapshot { session_id }`:** rich snapshot tailored for AI clients — current frame, scopes (resolved, depth-limited), source slice, breakpoints, watch values. One call returns enough context for an LLM to reason.

`GetStateSnapshot` is the AI-friendly primitive (per `12-ai-future.md`). It's NOT the same as `lazydap status` because it pre-resolves laziness: it actually fetches scopes and variables (depth-limited), grabs a source slice, includes recent step history. Heavyweight; not for polling.

## Multi-thread semantics

Most languages target single-threaded execution by default. C, Rust, Go, Python with threads, JS workers all complicate things.

In v0.1 we model multi-thread but the UI is thread-blind:

- `Threads { session_id }` returns the list. CLI exposes it.
- `stopped` events include `thread_id`. Daemon tracks per-thread state.
- Default `step` / `continue` apply to the stopped thread. `--all-threads` applies to all (per DAP `allThreadsStopped`).
- TUI initially shows one thread (the currently-stopped one). Multi-thread navigation post-v0.1.

## Session attachment vs launch

DAP distinguishes:

- `launch`: adapter spawns the debuggee.
- `attach`: adapter connects to an already-running process.

Both produce a session. lazydap supports both in v0.1, but the launch case is better-tested. Attach has more edge cases (PID lookup, permissions, "did we attach to the right process?").

## See also

- [`02-data-model.md`](02-data-model.md) — `Session`, `SessionState`, `PauseReason`
- [`04-protocol.md`](04-protocol.md) — IPC requests/responses for sessions
- [`10-async-to-sync.md`](10-async-to-sync.md) — `--wait` semantics during a session
