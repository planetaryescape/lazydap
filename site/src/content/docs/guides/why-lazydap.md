---
title: Why lazydap
description: The five trade-offs lazydap makes, what each buys, and when to use something else.
---

Decide whether lazydap fits before you install it. Every choice below has a defensible
opposite, and a real tool takes that opposite — so this page is mostly about what lazydap
gives up.

## The gap it fills

Debuggers are reachable three ways today: through an IDE, through a debugger's own REPL, or
through DAP if you already speak DAP. None of those is "run a command, read JSON."

That matters because a large share of the things that now want to inspect runtime state can
only run shell commands. An agent with a Bash tool cannot open VS Code. A CI job asserting
"this binary, given this input, stops at line 42 with `n == 10`" has no editor to drive. A vim
autocommand has no MCP host.

The research says the capability is worth having. Microsoft's debug-gym
([arXiv 2503.21557](https://arxiv.org/abs/2503.21557)) measured SWE-bench Lite success with
and without a debugger available to the model: +30% for Claude 3.7 Sonnet, +182% for o1, +160%
for o3-mini. Giving a model a debugger helps a lot. Giving it one it can actually invoke is
the unsolved half.

## The five trade-offs

### 1. Shell subcommands, not an MCP server

Anything that can run a command can drive lazydap: a Makefile, a CI job, a vim autocommand, an
agent whose only tool is Bash.

**What it costs.** No tool discovery, and no typed schema handed to your model by a host. An
MCP-native debugger gives you both, and the model never has to be told the commands exist.
Nearly every other project in this space is MCP-first for exactly that reason —
[`debugmcp/mcp-debugger`](https://github.com/debugmcp/mcp-debugger),
[`Govinda-Fichtner/debugger-mcp`](https://github.com/Govinda-Fichtner/debugger-mcp),
[`KashunCheng/dap_mcp`](https://github.com/KashunCheng/dap_mcp), and the go-delve org's own
`mcp-dap-server`.

**Why this way round.** MCP is one host protocol among several, and it is three years old.
Shell subcommands work in every host that exists, plus `cron`. An MCP bridge over lazydap's
socket is a thin separate crate if MCP wins; the reverse port is much harder.

**Take the opposite** if your agents live entirely inside one MCP host and you want the model
to discover the debugger rather than be told about it.

### 2. One blob per command, not an event stream

A stepping command returns after the program settles, with everything that happened in
between folded into the reply.

**What it costs.** A live feed shows you output the instant it appears. lazydap shows it to
you when the program next stops. For a dashboard that is worse.

**Why this way round.** A script has to decide what to do next, and deciding needs a settled
answer, not a stream it must reassemble. The [`--wait` contract](/guides/wait/) is that
settled answer.

**Take the opposite** if you are building a live UI. Though note that lazydap has the feed
too: a long-lived client can `Subscribe` on the socket, which is exactly what
[the TUI](/getting-started/tui/) does. The default is the blob; the stream is available.

### 3. The JSON is the contract, the table is not

`--format json` has a stable schema and breaking it costs a decision-log entry. `--format
table` is for your eyes and gets reflowed whenever it reads badly.

**What it costs.** Two output paths to maintain, and a reader who parses the pretty one gets
burned.

**Why this way round.** Tools that ship one pretty output and tell you to parse it are making
the opposite bet, and it works right up until the release that changes a column.

**Take the opposite** if you have one human user and no scripts.

### 4. It wraps codelldb rather than being a debugger

DWARF parsing, ptrace, evaluating an expression in the debuggee's language: LLDB does all of
that already, better than a rewrite would.

**What it costs.** codelldb's quirks become yours —
[eight of them are written down](/reference/codelldb-quirks/) — along with bugs lazydap
cannot fix because they live a layer down.

**Why this way round.** The porcelain/plumbing split is not new: lazygit over git, `httpie`
over curl, `kubectl` over the Kubernetes API. Calling any of them "just a wrapper" is correct
and beside the point. The value is not new capability, it is capability reachable from
somewhere new.

The "couldn't a 50-line script do this?" version is worth pricing honestly. It is not 50
lines. codelldb speaks DAP over TCP rather than stdio and announces its port on stderr only
when `RUST_LOG` is set; DAP frames are `Content-Length`-delimited; launch requires an
`initialized` / `setBreakpoints` / `configurationDone` ordering dance; requests correlate by a
monotonic `seq`; some adapters never send `terminated` so process death needs watching
directly; DAP sets `allThreadsStopped` only on the first stopped event. It is a few thousand
lines. Doing it once and letting everyone inherit it is the whole thesis.

**Take the opposite** if you need debugging semantics LLDB does not have.

### 5. One debug session per project at a time

The daemon is scoped to a project root and holds one session. Launching a second while the
first is live returns `SessionAlreadyActive`.

**What it costs.** Debugging four services at once needs a different tool today.

**Why this way round.** Session ids are in the protocol from the start, so multi-session stays
possible. Building it before anything else worked would have been the wrong order.

**Take the opposite** if simultaneous multi-process debugging is the job.

## When to use something else

- **You are happy in your IDE's debugger.** lazydap is not better than VS Code's debugger
  inside VS Code, and does not try to be. Microsoft's Copilot Debug Agent is the mature
  option there.
- **You want a TUI above all.** The TUI is real but deliberately second: it gets features
  after the CLI does, because the CLI is where they are defined.
- **You are debugging in a browser.** No CDP support, and none planned.
- **Python, Go or Node today.** codelldb only, which means C, C++, Rust and anything else
  LLDB handles. debugpy, delve and js-debug are planned.
- **You want it hosted.** There is no lazydap cloud. Nothing here phones home.

## What would prove this wrong

Worth stating, since a positioning page that cannot be falsified is marketing:

- MCP becomes the only channel agents get. Mitigated, not eliminated, by the socket being
  bridgeable.
- VS Code's Copilot Debug Agent leaves VS Code and becomes generally drivable.
- Record-and-replay debugging in the style of Replay.io becomes the norm, and stepping a live
  process stops being the interesting thing.

## See also

- [Architecture](/guides/architecture/) — how the client/daemon/adapter split is enforced
- [Debug with an agent](/guides/agents/) — the case this was built for
- [The `--wait` contract](/guides/wait/) — the design decision the whole thing rests on
