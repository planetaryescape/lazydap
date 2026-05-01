# Agent-driven debugging — what's out there, what's missing

Compiled from research conducted April 2026. Three parallel research agents searched for: AI-driven debugger projects, async-DAP-to-sync-shell patterns, and AI/ML enhancements applied to debugging. This article is the synthesis.

For the rapid-reference list of projects, see [`/docs/blueprint/11-state-of-the-art.md`](../blueprint/11-state-of-the-art.md). This article is the prose version — what the field actually looks like in 2026 and where lazydap fits. For the "isn't this just a wrapper on DAP?" question, see [`yes-its-a-wrapper.md`](yes-its-a-wrapper.md).

## The thesis, summarised

- **Live debugger driving by AI agents is rare in production.** Almost all "AI debugging" today is either static (PR review, suggesting bp locations) or instrumentation-based (inserting logs, observing telemetry). Actual stepping-and-inspecting is dominated by research projects.
- **MCP has captured the space's attention.** The half-dozen projects in this niche are MCP-tied. They work in Claude Code or Cursor; they don't work in shell scripts, CI, or any context that doesn't run an MCP host.
- **VS Code shipped the only mature commercial implementation.** [VS Code Copilot Debug Agent](https://code.visualstudio.com/docs/copilot/guides/debug-with-copilot) drives a real DAP session, sets tracepoints, analyses telemetry. But you have to be running VS Code.
- **The "shell-first" niche is empty.** Nobody has built `lazydap continue --wait --format json` style ergonomics for AI agents. Closest existing primitives are `pygdbmi` (gdb/MI) and the LLDB Python API; both are too low-level for agent consumption.
- **The thesis works.** Microsoft's [debug-gym](https://arxiv.org/abs/2503.21557) reports +30% to +180% on SWE-bench Lite when LLMs have pdb available. So runtime debugger access genuinely helps. The demand-side is real; the gap is purely UX.

## The taxonomy

### MCP-DAP wrappers

Closest in scope to lazydap, but accessed via MCP, not shell.

- **[debugmcp/mcp-debugger](https://github.com/debugmcp/mcp-debugger)** — TypeScript. Multi-language adapter registry. Most polished, ~100 stars, active.
- **[Govinda-Fichtner/debugger-mcp](https://github.com/Govinda-Fichtner/debugger-mcp)** — Rust + Tokio. 5 languages. Early-stage.
- **[KashunCheng/dap_mcp](https://github.com/KashunCheng/dap_mcp)** — Python. debugpy + lldb.
- **[go-delve/mcp-dap-server](https://github.com/go-delve/mcp-dap-server)** — Community project under go-delve org. Excellent reference for async-to-sync bridging (their `tools.go` continue handler is the closest existing analogue to what lazydap will do).

### VS Code-tied

- **[jasonjmcghee/claude-debugs-for-you](https://github.com/jasonjmcghee/claude-debugs-for-you)** — MCP + VS Code extension. ~507 stars (highest in the AI-debug space). Best-known of the lot. Requires VS Code running.
- **[microsoft/DebugMCP](https://github.com/microsoft/DebugMCP)** — VS Code extension exposing the running editor's debug session as MCP on port 3001. ~321 stars.
- **[VS Code Copilot Debug Agent](https://code.visualstudio.com/docs/copilot/guides/debug-with-copilot)** — official Microsoft. Drives DAP within VS Code.
- **[Visual Studio 2026 Copilot](https://devblogs.microsoft.com/visualstudio/visual-studio-2026-debugging-with-copilot/)** — Windows equivalent.

### IDE-integrated AI debugging (mostly NOT live)

- **[Cursor Debug Mode](https://cursor.com/blog/debug-mode)** — instruments code with logs + tracepoints. Suggests conditional bps but doesn't drive them. Per [docs](https://syn-cause.com/blog/cursor-debug-mode-review): "AI currently can't view live variable memory or manipulate breakpoints directly."
- **[Cursor BugBot](https://cursor.com/bugbot)** — static PR review. Not runtime.
- **[JetBrains AI Assistant](https://www.jetbrains.com/help/idea/ai-assistant-in-jetbrains-ides.html)** — inlay hints. No active debugger control.
- **[Sentry Seer](https://sentry.io/changelog/seer-sentrys-ai-debugger-is-generally-available/)** — post-deploy autofix from telemetry. Different problem.
- **[Replay.io](https://replay.io)** — records sessions, AI queries the recording. Browser/Node only.
- **[Replit Agent 3+](https://blog.replit.com/automated-self-testing)** — autonomous run-fix-rerun loops. Print-debugging, not stepping.

### Research projects validating the thesis

- **[Microsoft debug-gym](https://github.com/microsoft/debug-gym)** — pdb-only, Python. RL/eval environment. **+30% (Claude 3.7), +182% (o1), +160% (o3-mini)** on SWE-bench Lite when pdb available. ([arXiv 2503.21557](https://arxiv.org/abs/2503.21557).)
- **[ChatDBG](https://arxiv.org/abs/2403.16354)** — LLM-as-debugger via function calls into pdb / LLDB / GDB / WinDbg.
- **[InspectCoder](https://arxiv.org/pdf/2510.18327)** — dual-agent system (Inspector picks bp locations + Analyser).
- **[FloridSleeves/LLMDebugger (LDB)](https://github.com/FloridSleeves/LLMDebugger)** — ACL'24, runtime trace-based refinement.
- **[RepairAgent](https://github.com/sola-st/RepairAgent)** — autonomous bug repair, FSM controller.

### CLI-first debugger primitives (no AI)

The closest existing prior art for "agent-friendly CLI surface":

- **[GDB/MI](https://sourceware.org/gdb/current/onlinedocs/gdb.html/GDB_002fMI.html)** — line-based machine interface. Parsable but not JSON. [pygdbmi](https://github.com/cs01/pygdbmi) wraps it for Python dicts.
- **[LLDB Python API](https://lldb.llvm.org/python_api/lldb.SBProcess.html)** — full SBAPI scriptable in Python.
- **[lldb-dap](https://github.com/llvm/llvm-project/tree/main/lldb/tools/lldb-dap), dlv dap, gdb -i dap** — headless DAP servers. Speak DAP, not subcommands.
- **[pesticide](https://sr.ht/~raiguard/pesticide/)** — TUI DAP frontend. Sourcehut, dormant.

## Why the gap exists

Three reasons existing solutions converge on MCP/IDE-tied:

**1. MCP captured the funding and attention.** The "AI tooling" narrative in 2025–2026 was about agents that ran inside Claude Code or Cursor. Building for a shell environment without an MCP host wasn't where investment went.

**2. Bridging async DAP to sync shell is non-trivial.** Most projects didn't invest in the design work. mcp-dap-server's continue handler is roughly:

```go
for {
    msg, err := ds.client.ReadMessage()
    switch m := msg.(type) {
    case *dap.StoppedEvent: return ds.getFullContext()
    case *dap.TerminatedEvent: return "Program terminated"
    // other events fall through, loop continues
    }
}
```

It works. It also drops `output` events (the program's stdout — usually exactly what an agent wants). It hangs forever with no timeout. Multi-thread `stopped` events are first-wins. None of these are wrong; they're just unfinished. The design space (timeouts, output buffering, race-free thread state) takes care, and most projects haven't invested.

**3. The TUI half is unclaimed.** Pesticide is the only DAP TUI in active reach, and it's sourcehut-dormant. lazygit/lazydocker proved you can build a beautiful TUI on a `lazy*` brand; nobody has done it for debuggers yet.

## The lazydap positioning

What lazydap offers that nothing else does:

1. **Shell subcommands + JSON output.** `lazydap break main.c:42` works in CI, scripts, vim, agents, or `bash`. No MCP host. No editor. No DAP knowledge.
2. **TUI parity with the CLI.** The TUI uses the same protocol as the agent skill. Not bolted on.
3. **Single binary, daemon-backed.** `cargo install lazydap` and you're done.
4. **Documented `--wait` semantics.** Per [`/docs/blueprint/10-async-to-sync.md`](../blueprint/10-async-to-sync.md). Default 30s timeout. Buffered intervening events. Race-free thread state. Coalescing for multi-thread stops. Synthetic terminated on adapter death.
5. **`.vscode/launch.json` import.** Drop-in usable in repos with existing VS Code debug setup.

What lazydap explicitly is NOT differentiating on:

- We're not better than VS Code's debugger inside VS Code.
- We're not faster than `gdb`.
- We're not more powerful than LLDB's Python API.
- We're not adding CDP / browser support.
- We're not building cloud features.

Narrow, sharp positioning: **the agent-friendly, TUI-first option for native debugging in the terminal.**

## What this means for the project

Three implications for how we build:

### 1. The CLI surface design is a 6-month project, not a checklist

Per [`/docs/articles/the-cli-is-the-product.md`](the-cli-is-the-product.md), the CLI is the product. Every subcommand's JSON output schema is a contract third parties will depend on. Don't treat them like flag-parsing exercises.

### 2. The `--wait` design is the differentiator

Documented in [`/docs/blueprint/10-async-to-sync.md`](../blueprint/10-async-to-sync.md). It's where lazydap diverges from mcp-dap-server. Get it right; ship the spec; let competitors copy us, not the other way around.

### 3. AI features are external

Per [`/docs/blueprint/12-ai-future.md`](../blueprint/12-ai-future.md). lazydap ships two primitives — streaming events API + `getStateSnapshot` — and lets the community build everything else. Inverting this (baking AI into core) ages badly: today's hot LLM is tomorrow's footnote. Today's hot agent platform (Claude Code, Cursor) may not be tomorrow's. The protocol outlasts both.

## What we're betting against

- **MCP becomes the only way agents talk to tools.** If true, lazydap loses. Mitigation: lazydap protocol can be wrapped as MCP in a thin separate crate — no architecture change needed. We build the substrate; community builds the MCP bridge if they want it.
- **VS Code's Copilot Debug Agent moves outside VS Code.** Possible. But Microsoft has institutional weight on VS Code; they're unlikely to ship a CLI-first alternative.
- **Replay-style time-travel becomes the norm.** Replay.io is genuinely interesting. It's also browser/Node only. Native time-travel debug requires `rr` or equivalent and isn't a substitute for lazydap; it's an orthogonal feature.

## What we're betting on

- **The shell remains a substrate developers want to script.**
- **Agents that can be driven via Bash are more durable than agents that require a specific host.**
- **A clean CLI + TUI + protocol is a 5-year asset.** AI tooling rotation is fast; the boring stable plumbing under it isn't.

— author note. April 2026.

## Sources

- [debugmcp/mcp-debugger](https://github.com/debugmcp/mcp-debugger)
- [Govinda-Fichtner/debugger-mcp](https://github.com/Govinda-Fichtner/debugger-mcp)
- [KashunCheng/dap_mcp](https://github.com/KashunCheng/dap_mcp)
- [go-delve/mcp-dap-server](https://github.com/go-delve/mcp-dap-server)
- [microsoft/DebugMCP](https://github.com/microsoft/DebugMCP)
- [jasonjmcghee/claude-debugs-for-you](https://github.com/jasonjmcghee/claude-debugs-for-you)
- [LLDB built-in MCP](https://lldb.llvm.org/use/mcp.html)
- [VS Code Copilot Debug Agent](https://code.visualstudio.com/docs/copilot/guides/debug-with-copilot)
- [Cursor Debug Mode](https://cursor.com/blog/debug-mode)
- [Microsoft debug-gym (paper)](https://arxiv.org/abs/2503.21557)
- [ChatDBG (paper)](https://arxiv.org/abs/2403.16354)
- [InspectCoder (paper)](https://arxiv.org/pdf/2510.18327)
- [Sentry Seer](https://docs.sentry.io/product/ai-in-sentry/seer/)
- [Replay.io](https://www.replay.io/)
- [pesticide TUI](https://sr.ht/~raiguard/pesticide/)
- [GDB Python events](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Events-In-Python.html)
- [LLDB SBProcess Python API](https://lldb.llvm.org/python_api/lldb.SBProcess.html)
- [DAP specification](https://microsoft.github.io/debug-adapter-protocol/specification.html)
