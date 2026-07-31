---
title: Why lazydap
description: The six trade-offs lazydap makes, who the tool is actually for, and when to use something else.
---

Decide whether lazydap fits before you install it. Every choice below has a defensible
opposite, and a real tool takes that opposite - so this page is mostly about what lazydap
gives up.

## The gap it fills

Debuggers are reachable three ways today: through an IDE, through a debugger's own REPL, or
through DAP if you already speak DAP. During 2026 a fourth way appeared - a small cluster of
tools that each independently wrapped a debugger daemon in shell commands for coding agents:
[debug-skill](https://github.com/AlmogBaku/debug-skill),
[debug-that](https://github.com/theodo-group/debug-that),
[debugger-cli](https://github.com/akiselev/debugger-cli),
[dapi](https://github.com/shmulc8/dapi). lazydap is a member of that cluster, not its
origin, and this page is honest about what distinguishes it inside the cluster as well as
outside it: the schema and exit codes are documented contracts rather than flags, the
`--wait` reply is a specified state machine rather than "some context", and the CLI is the
product rather than the plumbing under a skill.

That matters because a large share of the things that now want to inspect runtime state can
only run shell commands. An agent with a Bash tool cannot open VS Code. A CI job asserting
"this binary, given this input, stops at line 42 with `n == 10`" has no editor to drive. A vim
autocommand has no MCP host.

The research points the same way, with appropriate modesty. Microsoft Research's
[debug-gym](https://arxiv.org/abs/2503.21557) is an environment for studying agents that use
an interactive debugger, and their write-up reports "significant performance improvement" on
SWE-bench Lite when the agent can actually use debugging tools - while being blunt about the
ceiling: "Even with debugging tools, our simple prompt-based agent rarely solves more than
half of the SWE-bench Lite issues"
([Microsoft Research blog](https://www.microsoft.com/en-us/research/blog/debug-gym-an-environment-for-ai-coding-tools-to-learn-how-to-debug-code-like-programmers/)).

Microsoft's follow-up, [Debug2Fix](https://arxiv.org/html/2602.18571v2), added two findings
that shaped this tool: a cheap model with a debugger beat an expensive one without, and
exposing raw debugger commands to the model produced negligible gains - the improvement came
from a mediated interface that bundles context at every stop. That mediated interface is
what `--wait` is.

### Who it is actually for

Most developers ship most days without a debugger, and so do most agents. A print statement
and a rerun genuinely cover a lot of work. lazydap is not aimed at all of it. It is aimed at
the territory where that loop stops working: C and C++, where you manage memory by hand and a
crash destroys its own evidence; unsafe Rust; the native extension underneath a Python stack;
races that vanish the moment you add a print. In that territory "add a log and run it again"
is not a loop, it is a wall. That is the segment lazydap is built for, even if the rest of the
world finds it merely convenient.

### The segment is where it starts, not where it stops

That segment is who lazydap wins without an argument. It is not the whole reach, and the
reason is friction rather than need.

It is worth being honest about why most developers skip the debugger most of the time. It is
rarely that a print statement tells them more. It is that the debugger costs a small ceremony
the print does not: a launch config, breakpoints set by hand, the commands half-remembered,
the trip out of the editor and back. A print costs one line where you already are, so it wins
on effort even when it loses on information.

Drive the debugger through an agent and that ceremony is the agent's problem, not yours. The
part you do shrinks to asking, and at that price the old comparison flips. One
`continue --wait` hands back more than a print ever could - where it stopped, why, the locals,
every line it emitted - for the same sentence. Log points make it sharpest:
`break --log "x={x}"` is the print workflow itself, minus the edit, minus the rebuild, on a
release binary, with structured output you can pipe. lazydap does the thing print does, better,
before it adds anything print cannot.

I do not want to oversell that. It is not free. There is latency, because a daemon and an
adapter and a session have to exist first. There are tokens, because the agent runs a handful
of commands. And there is install, which a print never needs and which lazydap has not yet
made painless - the honest reason `brew install` and a one-line installer sit on the critical
path rather than in a someday pile.

So the reach is wider than the segment, but the order matters. The segment converts with no
persuasion. The friction argument is what pulls the rest of the market in behind it. Leading
with friction would be the mistake: "frictionless debugging for everyone" is a sentence any
competitor can say, and Cursor already says a stronger version by taking the debugger out of
the loop entirely. The claim only holds when it is concrete - one sentence in, one settled
answer out, no polling and no MCP host - which is the [`--wait` contract](/guides/wait/)
wearing a friction label.

## The six trade-offs

### 1. Shell subcommands, not an MCP server

Anything that can run a command can drive lazydap: a Makefile, a CI job, a vim autocommand, an
agent whose only tool is Bash.

**What it costs.** No tool discovery, and no typed schema handed to your model by a host. An
MCP-native debugger gives you both, and the model never has to be told the commands exist.
Nearly every other project in this space is MCP-first for exactly that reason  - 
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

**What it costs.** codelldb's quirks become yours  - 
[nine of them are written down](/reference/codelldb-quirks/) - along with bugs lazydap
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

### 6. The CLI is the product, not the plumbing under a skill

The nearest tools in this space lead with the agent skill; the binary underneath is
supporting infrastructure, documented for the agent that drives it. lazydap inverts that
bet: one binary with one documented contract serves the agent, the shell script, the CI
job and the TUI identically. Pipe `--format json` into `jq`; stream `--format jsonl` into
`while read`; clear every breakpoint with `--format ids | xargs`; branch on exit code `4`
in CI. The cost is real: an agent-only surface can be simpler and terser, because nothing
else ever has to compose with it, and a skill-first tool can redesign its output every week
without breaking anyone. Take the opposite if the only consumer you will ever have is one
agent reading one screen.

## When to use something else

- **You are happy in your IDE's debugger.** lazydap is not better than VS Code's debugger
  inside VS Code, and does not try to be. Microsoft's Copilot Debug Agent is the mature
  option there.
- **You want a TUI above all.** The TUI is real but deliberately second: it gets features
  after the CLI does, because the CLI is where they are defined.
- **You are debugging in a browser.** No CDP support, and none planned.
- **Node today.** codelldb, debugpy and delve cover C, C++, Rust, Python and Go; js-debug
  is next on the roadmap.
- **You want it hosted.** There is no lazydap cloud. Nothing here phones home.

## What would prove this wrong

Worth stating, since a positioning page that cannot be falsified is marketing:

- MCP becomes the only channel agents get. Mitigated, not eliminated, by the socket being
  bridgeable.
- A cluster member ships the same contract: if debug-skill or debug-that publishes a stable
  documented schema, an exit-code contract and a specified wait-blob - with their wider
  adapter coverage - the differentiation collapses to execution quality.
- An IDE vendor goes headless: VS Code's Copilot debugging or JetBrains Junie leaving the
  IDE would put a giant in this lane overnight.
- Cursor flips its Debug Mode bet from log instrumentation to a real debugger.
- Record-and-replay debugging in the style of Replay.io becomes the norm, and stepping a live
  process stops being the interesting thing.

## See also

- [Architecture](/guides/architecture/) - how the client/daemon/adapter split is enforced
- [Debug with an agent](/guides/agents/) - the case this was built for
- [The `--wait` contract](/guides/wait/) - the design decision the whole thing rests on
