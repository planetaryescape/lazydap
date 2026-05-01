# 09 — Skill

The agent skill (`lazydap.skill`) is how AI agents discover and use lazydap. Same shape as mxr's: a ZIP archive containing a `SKILL.md` quick reference and a `references/commands.md` full command index.

Skills are NOT a runtime component. They're documentation that an agent consumes once per session to learn what commands exist and how to use them. The agent then invokes lazydap exactly like a human would, via shell.

## Skill vs MCP — why we chose skill

Most "AI debugging" tools today are MCP-based (the Model Context Protocol). MCP has real strengths but specific costs that make it the wrong default for lazydap:

| MCP | lazydap skill |
|---|---|
| Long-lived stdio process per session | Stateless shell invocations |
| Requires MCP-host runtime (Claude Code, Cursor, custom) | Works in any agent that can run Bash |
| Ties tool surface to the host's protocol version | Tool surface = CLI surface, owned by lazydap |
| Bespoke schemas per tool | One conventions doc covers all subcommands |
| Hard to use from CI / scripts / non-AI contexts | Same surface every consumer uses |

We're not against MCP. The agent skill is the **default**, and the protocol is open enough that someone could build a `lazydap-mcp` bridge if they want. The bridge would just shell out to `lazydap` subcommands. Same as a Python script would. Same as a Slack bot would.

(See [`docs/articles/agent-driven-debugging.md`](../articles/agent-driven-debugging.md) for the full positioning.)

## Skill ZIP shape

```
lazydap.skill/                              ← ZIP file at repo root
├── SKILL.md                                ← top-level summary, quick reference
└── references/
    ├── commands.md                         ← full subcommand reference
    ├── examples.md                         ← worked examples
    ├── error-codes.md                      ← exit code + error type reference
    └── output-schemas.md                   ← JSON output schemas per command
```

## `SKILL.md` (top-level)

YAML frontmatter declares the skill name and trigger description. Body is the agent's "table of contents" — concise, link-heavy.

```markdown
---
name: lazydap
description: |
  Use lazydap CLI to debug a binary or script. Set breakpoints, step through code,
  inspect variables, evaluate expressions. lazydap wraps any DAP adapter (codelldb
  for C/C++/Rust, debugpy for Python, etc) and exposes operations as shell
  subcommands returning JSON. Use --wait on stepping commands for synchronous
  agent loops.
---

# lazydap

A scriptable, terminal-first debugger. CLI, JSON-over-Unix-socket protocol, multiple frontends.

## Quick reference

### Start a debug session
```bash
lazydap launch <binary> --stop-on-entry --format json
```

Returns: `{"session_id": "...", "state": "Paused", "frame": {...}}`.

### Set a breakpoint
```bash
lazydap break <file:line> [--condition "<expr>"] --format json
```

### Step / continue (always use --wait from agents)
```bash
lazydap continue --wait --format json     # block until next paused/exited/terminated
lazydap step --wait --format json
lazydap step-into --wait --format json
lazydap step-out --wait --format json
```

### Inspect
```bash
lazydap stack --format json
lazydap scopes --format json
lazydap eval "<expression>" --format json
```

### End the session
```bash
lazydap disconnect [--terminate]
```

## Workflows

- [Quick triage](references/examples.md#quick-triage) — set bps, run, inspect
- [Step through a known bug](references/examples.md#step-through-a-known-bug)
- [Capture stdout during a run](references/examples.md#capture-stdout)
- [Multi-frame stack inspection](references/examples.md#multi-frame-stack-inspection)

## Reference

- [Full command list](references/commands.md)
- [Error codes](references/error-codes.md)
- [Output schemas](references/output-schemas.md)
```

## `references/commands.md`

The single source of truth for the agent's tool surface. Every subcommand documented with:

- **Synopsis** — usage line
- **Description** — one paragraph, what it does
- **Arguments** — positional and flags, with types
- **Output** — JSON shape (link to `output-schemas.md`)
- **Examples** — at least one realistic invocation
- **Errors** — common failure modes

Auto-generation: clap's help text + a hand-curated examples section. The generation script lives in `crates/daemon/scripts/generate-commands-md.rs` and runs in CI to keep `commands.md` in sync.

## `references/examples.md`

Worked end-to-end examples. The agent should be able to copy these and adapt.

```markdown
## Quick triage

You're handed an error report: "the binary segfaults when given the test file".

```bash
# Start the session, paused at entry.
$ lazydap launch ./mybinary -- test_input.dat --stop-on-entry --format json
{"session_id":"01ABC...", "state":"Paused", "reason":"Entry", ...}

# Continue, expect to crash.
$ lazydap continue --wait --format json
{
  "state": "Paused",
  "reason": "Exception",
  "frame": {"name": "process_record", "source": "src/parse.c", "line": 87},
  "captured_output": [{"category": "Stderr", "output": "Segmentation fault\n"}]
}

# Inspect the crash site.
$ lazydap stack --format json
$ lazydap scopes --format json
$ lazydap eval "buf" --format json

# Disconnect.
$ lazydap disconnect --terminate
```

## Step through a known bug

You suspect bug is in `parse_token` around line 142.

```bash
$ lazydap launch ./mybinary -- corpus.txt --format json
$ lazydap break src/parser.c:142 --format json
$ lazydap continue --wait --format json
{ "state": "Paused", "reason": "Breakpoint", "frame": {"line": 142}, ... }

$ lazydap eval "tokens[pos]" --format json
$ lazydap step --wait --format json
$ lazydap eval "tokens[pos]" --format json    # see how it changed
```
```

Examples cover the 80% case. Don't pad with rare scenarios.

## Distribution

Per [`15-decision-log.md`](15-decision-log.md) O04 (proposed):

- The skill ZIP lives in the repo root: `lazydap.skill`
- `cargo install lazydap` does NOT install the skill (it's a separate concern)
- Users install the skill into their agent's skill directory:
  - Claude Code: `~/.claude/skills/lazydap/`
  - Cursor: similar
  - Manual: unzip and point the agent at it

We provide `lazydap install-skill --target <agent>` to do this for known agents (post-v0.1).

## Versioning the skill

Skill version tracks lazydap version. Major lazydap version → skill schema may change. The skill's `SKILL.md` includes a `lazydap_min_version: "0.1.0"` so agents can warn if there's a mismatch.

## What an agent should NOT have to know

- DAP. Ever. The skill talks lazydap, not DAP.
- The Unix socket. The CLI handles connection.
- Daemon lifecycle. Auto-spawn handles it.
- Adapter quirks. The CLI hides them where possible.

If an agent has to learn any of these, the CLI/skill is wrong.

## See also

- [`docs/articles/agent-driven-debugging.md`](../articles/agent-driven-debugging.md) — competitive landscape
- [`AGENTS.md`](../../AGENTS.md) — generic agent guidance
- [`06-cli.md`](06-cli.md) — CLI surface that the skill documents
