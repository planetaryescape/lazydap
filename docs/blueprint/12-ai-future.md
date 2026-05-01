# 12 — AI future

Forward-looking. AI capabilities that could plug into lazydap once the core is working. **None of this is in v0.1.** This doc captures the shape so post-v0.1 work doesn't paint into a corner.

## Principle: AI is an external client

We don't bake AI into the core protocol. AI features are clients of the same `lazydap` protocol everything else uses. This keeps lazydap focused, keeps the core ageing-resistant (today's hot LLM is tomorrow's footnote), and lets the community ship AI features at their own cadence.

What we DO ship in core to enable AI:

1. **Streaming events API** (`Subscribe { channels: [...] }`) — already in v0.1.
2. **`getStateSnapshot` command** — returns rich JSON tailored for one-shot LLM context. New in v0.2 if useful, deferred otherwise.

Both are useful for non-AI clients too. The AI usage is just one consumer.

## Existing AI debug products (the landscape today)

Quick recap (full detail in [`11-state-of-the-art.md`](11-state-of-the-art.md)):

| Product | What it does | Live debug? |
|---|---|---|
| **Sentry Seer** | Post-deploy root cause + autofix from telemetry | No (post-hoc) |
| **Cursor Debug Mode** | Instruments code with runtime logs, generates hypotheses | No (instrumentation) |
| **Cursor BugBot** | Static PR review | No |
| **VS Code Copilot Debug Agent** | Sets bps + tracepoints, drives a session, analyses telemetry | **Yes** (within VS Code) |
| **Visual Studio 2026 Copilot** | Same on Windows | **Yes** (within VS) |
| **JetBrains AI Assistant** | Inlay hints explaining runtime errors | No |
| **Replay.io** | Records sessions, AI queries the recording | No (post-hoc) |
| **Replit Agent 3** | Run / observe / fix loop | No (print debugging) |
| **debug-gym** (research) | LLM drives pdb | **Yes** (research) |
| **ChatDBG** (research) | LLM as gdb/lldb client | **Yes** (research) |
| **InspectCoder** (research) | Dual-agent: Inspector picks bps + Analyser | **Yes** (research) |

Live debugger driving is rare in production. lazydap aims to make it easy, by being the protocol substrate.

## Capability roadmap

These plug into lazydap. Sequenced by complexity. Each row: what user sees / how it works / dependencies / where to look.

### Tier 1 — easy wins, ship in v0.2

#### "Why did this break?" — exception explanation

User stops on an exception. Press `?` (or run `lazydap explain --format json` from agent), get a 3–5 sentence explanation in plain English.

**How:** External tool reads `getStateSnapshot` (current frame, locals recursive depth-2, source ±20 lines, exception details). Single LLM call. Display as a popup in TUI or as JSON output.

**Existing:** Sentry Seer does this for production stack traces. lazydap version applies to live local sessions.

**Tech:** ~200 LoC bash/python script + Anthropic/OpenAI/local model.

**Caveat:** PII leakage. Code state may contain tokens, customer data. Document the warning. Allow `--local-llm` to use Ollama.

#### Stack trace summarisation

User pauses on an exception with 50 stack frames. Default: `lazydap stack` shows all 50. With AI: `lazydap stack --summarize`, returns the 3 frames most likely to be where user code went wrong.

**How:** `getStateSnapshot` + LLM ranks frames by "likely user code vs framework." Frame heuristics (path matches `target/` or `node_modules/`) preprocess.

**Existing:** [FaR-Loc paper](https://arxiv.org/abs/2509.20552) (66.9% Top-1 fault localisation from stack traces).

#### Auto-watch suggestion

When the user steps through a parser, the agent notices and offers: "watch `tokens` and `position`?"

**How:** Subscribe to `step` events with locals delta. Heuristic + LLM ranking on locals that change frequently.

**Latency budget:** must be ≤200ms to avoid feeling annoying. Local 7B model probably enough.

### Tier 2 — moderate, v0.3

#### Generate regression test from paused state

Paused at the bug. Agent sees state, source, recent steps. Generates a unit/integration test that reproduces the bug.

**How:** `getStateSnapshot` + project root + LLM with framework detection. Writes test file to disk (with confirmation).

**Existing:** RepairAgent does this incidentally during repair.

**Open questions:** Per-language test scaffold. How to discover the test framework. Apply vs preview vs save-as-suggestion.

#### "Fix it" patch suggestion

Paused at the bug. Agent proposes a code change.

**How:** `getStateSnapshot` + diagnosis from previous "explain" call + LSP integration for patch context. Generate diff, present in TUI for accept/reject.

**Existing:** Sentry Seer Autofix; Cursor; debug-gym.

**Caveat:** Apply vs preview. Rollback story. Don't auto-apply.

### Tier 3 — high complexity, v0.4+

#### "Set breakpoints to find this bug"

User describes a symptom in English. Agent picks bp locations, runs the program, observes.

**How:** Either (a) Inspector-agent pattern from InspectCoder, or (b) LLM with raw source + heuristic. (a) is more robust but heavier; (b) is cheaper but less reliable.

**Open questions:** How much project context to provide. Whether to expose a real CFG or trust the LLM. Cost ceiling per session.

#### Autonomous debug agent

User says "find this bug." Agent runs to completion, reports findings.

**How:** Full lazydap protocol via MCP wrapper + sandbox. Reasoning loop with bounded steps. Convergence detection (when does it give up?).

**Existing:** debug-gym is the reference. Cost is real (~270k tokens / bug for RepairAgent).

**Safety:** Running user code in loops is risky. Need sandboxing or explicit user OK per session.

### Tier 4 — research-grade, no timeline

- **Time-traveling debug + AI** — record execution, AI queries the recording. Replay.io shows it works for browsers; native equivalents need rr or rr-like infrastructure.
- **"Walk me through this"** — LLM narrates each step in plain English while you debug. Live, real-time. Latency-bound. Worth trying when models get faster.
- **"Find the inflection point"** — LLM watches a long-running session, flags moments where state diverges from expected invariant. Closest existing: Replay queries.
- **Variable naming inference** — opt-out only: rename `it` to `current_token` based on usage. Display-only overlay; doesn't modify code.

## Architecture for AI extension

### Two primitives in core, everything else external

#### 1. Streaming events API (already in v0.1)

```
Subscribe { channels: [Stopped, Output, BreakpointUpdated, ...] }
```

Anything observing a session uses this. AI advisors (Tier 1+) all consume it.

#### 2. `getStateSnapshot` command (target v0.2)

```rust
Request::GetStateSnapshot {
    session_id: SessionId,
    depth: u32,                      // variable expansion depth, default 2
    source_radius: u32,              // lines around current line, default 20
}

Response::StateSnapshot(StateSnapshot)

pub struct StateSnapshot {
    pub session_id: SessionId,
    pub session_state: SessionState,
    pub current_thread: Option<ThreadId>,
    pub current_frame: Option<StackFrame>,
    pub stack_trace: Vec<StackFrame>,
    pub scopes: Vec<ScopeWithVariables>,    // pre-resolved, depth-limited
    pub source_slice: Option<SourceSlice>,  // ±N lines around current line
    pub recent_steps: Vec<StepHistoryEntry>, // ring buffer of last N steps
    pub breakpoints: Vec<SourceBreakpoint>,
    pub watches: Vec<WatchValue>,
    pub captured_output: Vec<OutputChunk>,   // recent chunks
    pub timestamp: SystemTime,
}
```

This is THE LLM-friendly primitive. One call → enough context to reason. Heavyweight; not for polling.

`source_slice` shape:

```rust
pub struct SourceSlice {
    pub source: PathBuf,
    pub current_line: u32,
    pub start_line: u32,
    pub end_line: u32,
    pub lines: Vec<String>,
}
```

`recent_steps` shape — ring buffer, daemon-maintained:

```rust
pub struct StepHistoryEntry {
    pub timestamp: SystemTime,
    pub action: StepAction,                  // Continue / Step / StepIn / StepOut
    pub from_frame: Option<StackFrame>,
    pub to_frame: Option<StackFrame>,
    pub locals_delta: Vec<LocalsDeltaEntry>, // changed variables
}
```

This is what most existing DAP-MCP bridges reinvent badly. Ship it once, every AI client benefits.

### Extension surface (deliberately external)

Everything beyond the two primitives is third-party:

- An **MCP bridge** crate that wraps `lazydap` subcommands as MCP tools — written by us or community.
- An **`lazydap-advisor`** companion binary that subscribes, calls LLMs, posts back as comments. Written by anyone.
- **Custom skills** beyond `lazydap.skill` — e.g., `lazydap-claude-skill-pro` with project-specific tuning.
- **HTTP/WebSocket bridge** for browser frontends.
- **Language-specific helpers** (`lazydap-rust-helper` that adds Rust-specific features atop the generic core).

We ship none of these in core. We ship the protocol that enables them.

## What we explicitly won't ship

- **An LLM inside the daemon.** Daemon stays small. AI is opt-in, external.
- **A vector DB / embeddings store.** Out of scope. Use LSP / project indexers if you want symbol search.
- **A bundled API key system.** Users bring their own keys.
- **AI-driven mutation by default.** All mutations stay `--dry-run`-able and behind explicit user action.

## Local vs cloud LLMs

Code state contains secrets / PII. Running an LLM on debug state means leaking. We don't ship local model integration in core (see "what we won't ship") but we make local models easy to use:

- All AI primitives work with any LLM endpoint.
- Default integration recipes for Ollama / LM Studio / Llama.cpp documented in `docs/articles/`.
- `--local-only` flag on AI advisor commands prevents accidental cloud calls.

State of local models for debug tasks (per April 2026 research):

- **Stack trace summarisation** / **frame ranking**: 7B–14B is plenty.
- **Root cause hypothesis on novel code**: 32B+ is the floor; quality drop from frontier is real but acceptable.
- **Autonomous repair**: still well behind frontier; SWE-bench-Pro shows 60% of "solved" SWE-bench-Verified had solution leakage. Real-world is harder.

## Sequencing

| Tier | When | Effort | Open Questions |
|---|---|---|---|
| 1 — explain, summarise stack, auto-watch | v0.2 (~M20-ish) | Low — single LLM call each | Latency budgets, format of frame ranking |
| 2 — generate test, fix patch | v0.3 | Medium — write tooling | Per-language scaffolds |
| 3 — set bps, autonomous | v0.4+ | High — research-grade | CFG vs raw source, cost ceiling |
| 4 — narrate, time-travel, infer names | post-v0.5 | Research | Many |

Ship the primitives in v0.2. Let the community build Tier 1 features. Watch which ones gain traction. Ship Tier 2 ourselves where the ergonomics matter.

## See also

- [`11-state-of-the-art.md`](11-state-of-the-art.md) — what exists today
- [`docs/articles/agent-driven-debugging.md`](../articles/agent-driven-debugging.md) — research summary
- [`15-decision-log.md`](15-decision-log.md) — D023 (AI external)
