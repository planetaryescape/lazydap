# 11 — State of the art

What exists today in the AI-driven / agent-driven / scriptable debugger space, and where the gap is. Compiled from research conducted April 2026.

## TL;DR

There's a real gap. Every existing solution is **MCP-tied** (assumes Claude/Cursor host), **VS-Code-tied** (assumes the editor is running), or **DAP-tied** (assumes you already speak DAP). Nothing exposes debugging as plain shell subcommands an agent can invoke via Bash. The strongest direct competitor is `mcp-debugger` (TypeScript, ~100 stars). lazydap's positioning: shell-first, JSON output, no MCP runtime needed, TUI parity.

## Direct competitors

### MCP-DAP wrappers

These bridge DAP to the Model Context Protocol. Closest in scope to lazydap's daemon, but accessed only via MCP — not Bash.

- **[debugmcp/mcp-debugger](https://github.com/debugmcp/mcp-debugger)** — TypeScript. Adapter registry pattern: codelldb (Rust/C/C++), debugpy (Python), js-debug (Node), Delve (Go), JDI (Java), netcoredbg (.NET). Most polished of the lot. ~100 stars, active. **Direct competitor in spirit.**
- **[Govinda-Fichtner/debugger-mcp](https://github.com/Govinda-Fichtner/debugger-mcp)** — Rust + Tokio. 5 languages. ~5 stars, early-stage. Similar architecture to what lazydap will be.
- **[KashunCheng/dap_mcp](https://github.com/KashunCheng/dap_mcp)** — Python. debugpy + lldb. ~37 stars.
- **[go-delve/mcp-dap-server](https://github.com/go-delve/mcp-dap-server)** — Community project under go-delve org. Reference for the request-response loop pattern (their `tools.go` `continue` handler is the closest existing analogue to our `--wait`).
- **[mizchi/debugger-mcp](https://github.com/mizchi/debugger-mcp)** — WIP.
- **[microsoft/DebugMCP](https://github.com/microsoft/DebugMCP)** — VS Code extension exposing the running editor's debug session as MCP on port 3001. Requires VS Code running. ~321 stars (high because of Microsoft branding).

### VS Code-tied

These rely on VS Code being running to drive its debug session.

- **[jasonjmcghee/claude-debugs-for-you](https://github.com/jasonjmcghee/claude-debugs-for-you)** — MCP + VS Code extension. ~507 stars (highest in the AI-debug space). Language-agnostic via existing `launch.json`. **Strong adjacent competitor**, but VS Code-bound.
- **[withpointbreak/pointbreak-claude](https://github.com/withpointbreak/pointbreak-claude)** — Claude Code plugin for breakpoint debugging. VS Code-tied.
- **[VS Code Copilot Debug Agent](https://code.visualstudio.com/docs/copilot/guides/debug-with-copilot)** — official Microsoft tooling. Instruments tracepoints + conditional breakpoints, runs sessions, analyzes telemetry. **Strong direct competitor within the VS Code ecosystem.**
- **[Visual Studio 2026 Debugging with Copilot](https://devblogs.microsoft.com/visualstudio/visual-studio-2026-debugging-with-copilot/)** — Windows-tied equivalent.

### Native-debugger MCP wrappers (gdb / lldb directly)

- **[smadi0x86/MDB-MCP](https://github.com/smadi0x86/MDB-MCP)** — wraps GDB and LLDB for binary debugging.
- **[LLDB built-in MCP](https://lldb.llvm.org/use/mcp.html)** — official LLDB MCP support. First-class debugger vendor support. Adjacent: only LLDB, only MCP.
- **[stass/lldb-mcp](https://playbooks.com/mcp/stass/lldb-mcp)**, **[AgentSmithers/x64DbgMCPServer](https://github.com/AgentSmithers/x64DbgMCPServer)** — niche wrappers.

### Browser CDP MCPs (different debugger, similar pattern)

- **[ChromeDevTools/chrome-devtools-mcp](https://github.com/ChromeDevTools/chrome-devtools-mcp)** — official Chrome MCP. Sets breakpoints, inspects vars, steps, watches network. ~31K stars. Adjacent (browser-only) but most production-grade in the space.

## Adjacent but different

### IDE-integrated AI debugging (mostly suggestion-based)

- **[Cursor Debug Mode](https://cursor.com/blog/debug-mode)** (v2.2, late 2025). Instruments code with runtime logs + tracepoints, generates hypotheses, asks user to repro. **Per [docs](https://syn-cause.com/blog/cursor-debug-mode-review): "AI currently can't view live variable memory or manipulate breakpoints directly"** — it suggests conditional breakpoints but doesn't drive them. **Not actually live debugging.**
- **[Cursor BugBot](https://thelinuxcode.com/what-is-cursor-bugbot-a-practical-firstperson-guide-to-ai-debugging-in-cursor/)** — static logical bug detection. Not runtime.
- **[JetBrains AI Assistant](https://www.jetbrains.com/help/idea/ai-assistant-in-jetbrains-ides.html)** — explains runtime errors via inlay hints. No active debugger control.
- **[GitHub Copilot Spaces / Workspace](https://github.com/features/copilot)** — task-level. Not interactive debugger driving.
- **[Sentry Seer](https://sentry.io/changelog/seer-sentrys-ai-debugger-is-generally-available/)** — post-deploy autofix from telemetry. Different problem (production debugging from logs/traces, not local stepping).
- **[Replay.io](https://www.replay.io/)** — browser session recording + AI queries via MCP. Web/Node only. Architecturally interesting but different scope.
- **[Replit Agent 3+](https://blog.replit.com/automated-self-testing)** — autonomous run-fix-rerun loops, up to 200 minutes. Print-debugging, not stepping.

### CLI debuggers without AI integration

- **[GDB/MI](https://sourceware.org/gdb/current/onlinedocs/gdb.html/GDB_002fMI.html)** — line-based machine interface. Parsable but not JSON. [pygdbmi](https://github.com/cs01/pygdbmi) wraps it for Python. Inspiration for the agent-friendly CLI pattern, but not it.
- **[LLDB Python API](https://lldb.llvm.org/python_api/lldb.SBProcess.html)** — full SBAPI scriptable in Python. Can emit JSON for some queries.
- **[lldb-dap](https://github.com/llvm/llvm-project/tree/main/lldb/tools/lldb-dap), [dlv dap](https://github.com/go-delve/delve/blob/master/Documentation/api/dap/README.md), [gdb -i dap](https://www.sourceware.org/gdb/news/dap.html)** — headless DAP servers. Speak DAP, not subcommands.
- **[pesticide](https://sr.ht/~raiguard/pesticide/)** — TUI DAP frontend (sourcehut, low activity).
- **[Seer](https://github.com/epasveer/seer)** — Qt6 GUI on GDB/MI. Adjacent (GUI, not CLI).
- **pdb / ipdb / pdbpp** — interactive REPL only, no JSON output, no subcommand surface.

### Lazy* family (the cousin tools)

`lazygit`, `lazydocker`, `lazysql`, `lazynpm`, `lazyjournal`, `lazycrate`, `lazyrestic` — none have debug integration. No `lazydap` exists. The `lazy*` naming pattern is unused for debuggers. **Real gap.**

## Research projects worth knowing about

These don't ship as products, but they validate the demand-side thesis: runtime debugger access materially helps LLM coding performance.

- **[Microsoft debug-gym](https://github.com/microsoft/debug-gym)** — pdb-only, Python only, training/evaluation environment. Reports +30% (Claude 3.7), +182% (o1), +160% (o3-mini) on SWE-bench Lite when pdb available. **Validates the thesis.** [arXiv 2503.21557](https://arxiv.org/abs/2503.21557).
- **[ChatDBG](https://arxiv.org/abs/2403.16354)** — LLM-as-debugger via function calls into pdb / LLDB / GDB / WinDbg. Most directly analogous to "what an agent could do with lazydap."
- **[InspectCoder](https://arxiv.org/pdf/2510.18327)** — dual-agent system (Inspector picks bp locations + analyser). Cites +5–60% repair accuracy. Ships InspectWare middleware.
- **[FloridSleeves/LLMDebugger (LDB)](https://github.com/FloridSleeves/LLMDebugger)** — ACL'24, runtime trace-based refinement.
- **[AgentStepper](https://arxiv.org/html/2602.06593v1)** — breakpoints/stepping for LLM agents themselves (debugs agents, not user code). Adjacent.
- **[RepairAgent](https://github.com/sola-st/RepairAgent)** — autonomous bug repair, FSM controller. ICSE 2025.

## Why the gap exists

Three reasons existing solutions converge on MCP/IDE-tied:

1. **MCP is fashionable.** It's where the funding and attention go in 2025–2026.
2. **DAP is hard.** Wrapping it well requires the kind of design work this blueprint represents — most projects don't invest in the sync/async bridge, the TOML state model, the multi-session-from-day-1 protocol.
3. **The TUI half is unclaimed.** Pesticide is the only DAP TUI in active reach, and it's sourcehut-dormant. `pesticide` was a genuine attempt; it didn't reach critical mass. lazydap's TUI-as-second-class-citizen positioning is novel.

## What lazydap offers that nothing else does

1. **Shell subcommands + JSON.** No MCP host, no editor, no DAP knowledge required. `lazydap break main.c:42` works in CI, scripts, vim, agents, or `bash`.
2. **TUI parity.** The TUI uses the same protocol as everything else. Not bolted on.
3. **Single binary, daemon-backed.** No microservices, no orchestration. `cargo install lazydap` and you're done.
4. **`--wait` semantics.** Documented, principled bridge between push DAP and pull shell. Most existing wrappers either don't have it (mcp-dap-server's loop is the closest) or hand-wave it.
5. **`.vscode/launch.json` import.** Drop-in usable in any repo with existing VS Code debug setup.

## What lazydap will NOT differentiate on

We're not better than VS Code's debugger inside VS Code. We're not faster than `gdb`. We're not more powerful than LLDB's Python API. We're not adding CDP / browser support. We're not building cloud features.

We're **the agent-friendly, TUI-first option for native debugging in the terminal.** Narrow, sharp.

## Risk: someone else builds it first

`debugmcp/mcp-debugger` is closest. If they pivot to expose a CLI surface alongside MCP, the gap narrows fast. Mitigations:

- Ship M0–M5 quickly. CLI debugger that works is more useful than blueprint that doesn't.
- TUI quality matters. lazydap's TUI is a real differentiator if it's good.
- Multi-language adapters from M18 onward.
- AI advisor extension points (per [`12-ai-future.md`](12-ai-future.md)) make lazydap a platform, not a tool.

## Sources

Research conducted via parallel web search agents, April 2026.

- [debugmcp/mcp-debugger](https://github.com/debugmcp/mcp-debugger)
- [Govinda-Fichtner/debugger-mcp](https://github.com/Govinda-Fichtner/debugger-mcp)
- [KashunCheng/dap_mcp](https://github.com/KashunCheng/dap_mcp)
- [go-delve/mcp-dap-server](https://github.com/go-delve/mcp-dap-server)
- [microsoft/DebugMCP](https://github.com/microsoft/DebugMCP)
- [jasonjmcghee/claude-debugs-for-you](https://github.com/jasonjmcghee/claude-debugs-for-you)
- [LLDB built-in MCP](https://lldb.llvm.org/use/mcp.html)
- [VS Code Copilot Debug Agent docs](https://code.visualstudio.com/docs/copilot/guides/debug-with-copilot)
- [Cursor Debug Mode](https://cursor.com/blog/debug-mode)
- [Microsoft debug-gym (paper)](https://arxiv.org/pdf/2503.21557)
- [ChatDBG (paper)](https://arxiv.org/abs/2403.16354)
- [InspectCoder (paper)](https://arxiv.org/pdf/2510.18327)
- [Sentry Seer](https://docs.sentry.io/product/ai-in-sentry/seer/)
- [pesticide TUI](https://sr.ht/~raiguard/pesticide/)
