# Yes, it's a wrapper

> "Isn't lazydap just a wrapper on DAP?"

Yes. So is every debugger UI you've ever used.

VS Code's debugger wraps DAP. CLion wraps it. Xcode's debug pane wraps LLDB. `gdbgui` wraps GDB. JetBrains' debugger wraps DAP and platform-specific protocols. The question isn't whether lazydap is a wrapper. **Every usable debugger surface is a wrapper.** DAP is a protocol designed for IDE plugins to drive debuggers. Humans don't use it. Agents can't use it. Shell scripts won't use it. It's plumbing.

The question is what the porcelain is for.

## Plumbing and porcelain

Git uses this terminology officially. There's git plumbing — `git hash-object`, `git update-index`, `git ls-tree` — operations that compose the data model. Then there's porcelain — `git commit`, `git checkout`, `git rebase` — the commands you actually use. Plumbing is what makes git work. Porcelain is what makes git usable.

The pattern shows up everywhere:

| Plumbing | Porcelain |
|---|---|
| Filesystem syscalls + patch format | `git` |
| Git plumbing | `lazygit`, `tig`, `magit` |
| HTTP libraries | `curl`, `httpie` |
| Docker daemon API | `docker compose`, `lazydocker` |
| Kubernetes API server | `kubectl`, `k9s` |
| AWS REST APIs | `aws` CLI |
| Compiler diagnostics | LSP servers |
| LSP | Editor LSP clients |
| **DAP** | **lazydap** |

Anyone calling lazygit "just a wrapper on git" is correct and missing the point. Same for `httpie` on `curl`. Same for `kubectl` on the K8s API. The point of porcelain isn't to add capability that the plumbing doesn't have. **The point is to expose the plumbing's capability through an interface humans, agents, and scripts can actually use.**

## What the lazydap porcelain does that DAP doesn't

Four things, none of which DAP is designed for:

### 1. Shell access

DAP is a stateful, push-based protocol over a long-lived stdio pipe. Shell scripts and AI agents are stateless: invoke a command, get a response, exit. Bridging the two requires a daemon that holds the DAP session and a CLI that translates each invocation into the right protocol exchange.

After lazydap, debugging from the shell looks like this:

```bash
lazydap launch ./mybinary --stop-on-entry
lazydap break src/parser.c:142
lazydap continue --wait | jq '.captured_output'
lazydap eval "tokens[pos]"
```

Without lazydap, none of that works. DAP can't be invoked this way.

### 2. The async-to-sync bridge — `--wait`

DAP fires `stopped`, `output`, `breakpoint`, `thread` events asynchronously. A shell tool wants one response per command. Bridging these correctly is non-trivial:

- Block until the next *stable state* (paused, exited, terminated).
- Buffer intervening events (program's stdout, breakpoint changes, thread updates) into the response.
- Handle multi-threaded targets where multiple `stopped` events arrive in rapid succession.
- Detect adapter death and emit a synthetic `terminated`.
- Apply timeouts that don't leave the program stuck.
- Avoid race conditions where state mutations get clobbered by stale events.

Nobody else has documented or shipped this well. The closest existing implementation, `mcp-dap-server`, hangs forever (no timeout), drops `output` events (which is usually what the agent wants), takes only the first stopped event in multi-thread cases, and has no race-condition protection. lazydap's spec is in [`/docs/blueprint/10-async-to-sync.md`](../blueprint/10-async-to-sync.md). It's the longest blueprint doc by intent, because every ambiguity here costs months of bug reports.

This isn't wrapping. **This is the product.**

### 3. State that DAP doesn't own

DAP describes one debug session. It doesn't describe:

- Persistent breakpoints across sessions
- Watch expressions that survive restart
- Named launch configurations per project
- A daemon that auto-spawns and manages multiple clients
- `.vscode/launch.json` interop
- Project root detection
- Adapter binary discovery
- Crash recovery

All of that lives in lazydap. None of it lives in DAP, by design — DAP is concerned with one debug session, period. Everything around the session is the wrapper's job.

### 4. A protocol other things can build on

DAP assumes one IDE-style client per session. lazydap exposes its own protocol (JSON over Unix socket) that supports many concurrent clients, each subscribing to whichever events they care about. This unlocks:

- A TUI for terminal-first developers
- An agent skill for AI tools (Claude Code, Cursor, custom)
- An Electron desktop app for those who want one
- A web bridge for browser-based debugging dashboards
- An MCP server (a thin wrapper layered on top, for those who want MCP)
- Vim plugins, language bindings, custom scripts in any language

None of these existed before because DAP can't be that substrate. lazydap can.

## "But couldn't a 50-line Python script do this?"

Try it.

To debug a C program from a shell using only DAP:

1. Spawn `codelldb` as a child process.
2. Parse "Listening on port N" from stderr (codelldb is TCP-only — surprise).
3. Connect via TCP.
4. Frame `Content-Length: N\r\n\r\n` headers around JSON bodies.
5. Send `initialize`. Parse the `Capabilities` response.
6. Send `launch`. **Don't await its response yet** — wait for the `initialized` event first.
7. Send `setBreakpoints` with the source file's full breakpoint list.
8. Send `configurationDone`.
9. *Now* the `launch` response arrives.
10. Manage a monotonic `seq` counter for request/response correlation.
11. Continuously read incoming messages. Route responses by `request_seq`. Dispatch events to handlers.
12. Buffer `output` events, don't lose them under high throughput.
13. On `stopped`, fetch `stackTrace` → `scopes` → `variables` lazily.
14. On `terminated`, clean up. Detect adapter death via SIGCHLD as a backstop because adapters sometimes don't send `terminated`.
15. Handle the disconnect race per DAP spec issue #126.
16. Persist breakpoints between sessions if you want them to survive.
17. Substitute `${workspaceFolder}` and friends if you want to read `.vscode/launch.json`.
18. Don't pipeline execution requests — queue them, or deadlock per ptvsd #1502.
19. Handle multi-thread `stopped` events. DAP only sets `allThreadsStopped: true` on the first.
20. ...and that's just to support `lazydap launch && lazydap break && lazydap continue --wait`.

It's not 50 lines. It's a few thousand. The DAP spec alone has roughly 50 message types and most have edge cases. Anyone who's written a DAP client will recognise this list — and they'll recognise that it's the work *every* DAP wrapper has to redo.

The lazydap thesis: **redo it once, well, and let everyone else inherit the work.**

## The empirical case

Microsoft published [debug-gym](https://arxiv.org/abs/2503.21557) in 2025. They measured what happens when LLMs are given runtime debugger access via pdb:

- Claude 3.7 Sonnet: **+30%** on SWE-bench Lite
- o1: **+182%**
- o3-mini: **+160%**

These are not small numbers. Runtime debugger access materially improves LLM coding performance. The catch: those gains are locked behind pdb (Python only) inside a research framework. There's no shipping product that gives an arbitrary AI agent shell-friendly debugger access for arbitrary languages.

That's the gap lazydap closes.

## The wrapper IS the product

If "wrapper" is the criticism, the answer is yes — and that's the point.

The lazydap pitch in three sentences:

> DAP is a protocol designed for IDEs to talk to debuggers. lazydap exposes that capability as shell subcommands returning JSON, plus a TUI, plus an agent skill — so anything that can run Bash can debug code, not just IDEs. The wrapper IS the product.

That works as an elevator pitch. It works as landing-page copy. It works as the answer to a hostile reviewer.

The criticism — "it's just a wrapper" — is correct in the same trivial sense that lazygit is just a wrapper on git, kubectl is just a wrapper on the K8s API, httpie is just a wrapper on HTTP. None of those are diminished by the description. Neither is lazydap.

Building wrappers that are themselves products is one of the most reliable patterns in developer tooling history. It's how `git` became usable (porcelain). It's how Kubernetes became usable (`kubectl`). It's how LLM tool calling became usable (LSP, MCP). The pattern works because protocols are designed to be powerful, and powerful protocols need usable surfaces. lazydap is one of those surfaces.

If lazydap fails, it won't be because it's a wrapper. It'll be because it's the wrong wrapper. That's a different conversation, and a more interesting one.

— author note. May 2026.
