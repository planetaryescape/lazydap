# The CLI is the product

There's a slow, quiet category-killer in dev tooling: making the CLI the *primary* surface, not a fallback or a nicety bolted on after the GUI ships.

The current orthodoxy is the opposite. You build a beautiful GUI/TUI/web app, it has runtime state, the GUI owns the runtime state, and CLI commands are scripts that have to claw their way into that state somehow — usually via an API the GUI grudgingly exposes. The result: the GUI is the product, the CLI is the second-class citizen, and anyone trying to script around it is fighting the architecture.

lazydap inverts that. The CLI is the product. Every operation goes through a JSON-over-Unix-socket protocol the CLI speaks. The TUI uses the same protocol. The agent skill uses the same protocol. An Electron app would use the same protocol. There is no privileged surface, no special TUI-only state, no "well, the CLI can do most things but for X you need the GUI."

This isn't about minimalism. It's about leverage.

## The leverage

When the CLI is the product, everything downstream gets cheap.

Want a web dashboard? Spawn a process per request, parse its JSON output, render. ~100 lines.

Want an Electron desktop app? Same. The Electron renderer process talks to the lazydap CLI as a subprocess.

Want a vim plugin? `vim.system(["lazydap", "continue", "--wait", "--format", "json"])`.

Want an MCP server? Wrap each subcommand as an MCP tool. ~200 lines.

Want CI to assert "this binary, given this input, exits at line X with var Y in this state"? Bash + jq.

Want to write your debugger UI in Haskell? Sure. `Process.callProcess`, parse JSON.

None of these need core lazydap to know about them. None of them need a plugin API. None of them break when lazydap upgrades, as long as the CLI's JSON contract is stable.

This is the same pattern that made `git` durable. Plumbing commands that produce stable, parseable output, porcelain commands built on top, third parties (gitg, lazygit, magit, vim-fugitive) building entirely outside the project. Git doesn't have to worry about lazygit existing. Lazygit doesn't have to fork git. They share an ABI: the CLI surface and its output format.

## Why most projects don't do this

Three reasons. None are good.

**1. The CLI is harder to make beautiful.** A GUI gets ooh-ahh credit for animations and gradients; a CLI gets credit for not crashing. So tools optimise for the demo, the demo is the GUI, the CLI rots.

This is solvable. lazydap's CLI is the demo. JSON-shaped output, auto-detected formatting, `--dry-run` previews, structured exit codes, completion scripts. Make the CLI feel good. Tools like `gh`, `jj`, `nu`, `mxr`, `httpie`, `lazygit` show that "beautiful CLI" is real and possible.

**2. Designing a stable protocol is hard.** It is. But the cost is paid once, at the start, and you reap the reward forever. The cost of NOT designing one — every UI is an island, every script is bespoke, every integration is a one-off — compounds for the lifetime of the tool.

**3. People conflate "the CLI is the product" with "no GUI."** Wrong. lazydap has a TUI. It's just a *client* of the protocol the CLI speaks. The TUI can be fantastic without being privileged.

## Inherited from mxr

This pattern is mxr's, applied to debugging.

mxr is a daemon-backed CLI email client by the same author. CLI subcommands first, JSON output by default for non-TTY, TUI as one of N possible clients. The choice was deliberate: agents (Claude Code, Cursor) couldn't drive an email client effectively when the email client was a TUI, but they could drive a CLI fluently. The mxr design got rewarded: agents read the skill manifest once, then invoked subcommands like a human would. No bespoke protocol, no special agent-mode, no tier-2 surface. Just shell commands that returned JSON.

Same exact bet for lazydap. Debugging happens to be the dev task agents care about most when they need runtime state. By making lazydap CLI-first, an agent debugging session is just a Bash script. No MCP runtime needed. No editor needed. No "AI plugin" architecture.

## What "CLI is the product" forces you to do well

Three discipline checks the architecture exerts:

**1. The protocol has to be stable.** Once a frontend (TUI, agent, web) speaks it, breaking changes break them all. So the JSON output is versioned. New requests are additive. Removed requests get a deprecation cycle.

This sounds like work. It is. But it's the work that makes everything else stop being work. Once the protocol is stable, three frontends can ship in parallel without coordinating with each other.

**2. Every TUI action has to have a CLI equivalent.** Inviolable. If there's a feature only in the TUI, the architecture has cracked — the TUI has reached past the protocol into private daemon state.

In practice, this is enforced by Cargo's dependency graph: the TUI crate cannot depend on the daemon crate. So it can't reach into daemon-private state even if a developer wants to. The crate boundaries become the contract.

**3. JSON output has to be a real product feature, not an afterthought.** Stable schemas. Auto-detection of TTY vs non-TTY. A `--format` flag for explicit choice. Consistent error shape. Real schema documentation.

Most CLI tools have a `--json` flag. Most are forgettable. lazydap's JSON has to be the canonical output — agents and scripts depend on it.

## The trade-offs

Honest about the costs:

- **CLI-first is slower up front.** Designing a protocol, then a CLI, then a TUI is more work than just a TUI. The reward is in compound interest, not week one.
- **The TUI ends up less integrated.** It can't peek at private state. It can't take shortcuts. Sometimes this means the TUI is slightly less elegant than a coupled equivalent. Worth it.
- **Some operations don't fit the model.** A truly bidirectional, real-time UX (like a video call) isn't well-served by request/response. Debugging, by luck or design, does fit: pause, inspect, step, inspect, repeat.

## When this pattern is wrong

Don't pick CLI-first if:

- The product really is a GUI; nobody'll ever script it.
- The output is fundamentally non-textual (image editing, music production).
- You're optimising for a single user, not a platform.

If you're optimising for being the substrate other people build on, CLI-first is the move. lazydap is optimising for that.

## The bet

lazydap bets that "the CLI is the product" beats "the agent integration is the product" or "the TUI is the product" over a 5-year horizon. AI tools come and go. MCP may or may not stick. VS Code's debugger surface may evolve. ratatui may be replaced by something better. But the JSON-over-Unix-socket protocol — the boring, unsexy, scriptable surface — should outlast all of them.

If we're right, in three years there are five frontends for lazydap and we wrote one. If we're wrong, lazydap shipped a clean CLI alongside a TUI, and that's not a bad place to land.

— author note. April 2026.
