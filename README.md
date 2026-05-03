# lazydap

A scriptable, terminal-first debugger. Wraps any [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/) adapter (codelldb, debugpy, js-debug, delve, ...) behind a CLI you can drive from any language, any frontend, any agent.

> **Status:** pre-alpha. No code yet. This repo currently holds the architecture, blueprint, and per-milestone implementation plan. Code arrives starting with milestone [M0](docs/implementation/tasks/M00-hello-adapter.md).

## What lazydap is

- **A daemon** that owns the DAP adapter process and the live debug session.
- **A CLI** (`lazydap`) that exposes every debug operation as a subcommand returning JSON.
- **A TUI** (`lazydap tui` or bare `lazydap`) that's just one client of that CLI.
- **A skill** (`lazydap.skill`) that lets agents drive the debugger using the same CLI a human would.
- **A protocol** (JSON over Unix socket) that anyone can build a frontend on — Electron, web, MCP server, vim plugin, custom HTTP bridge, language bindings.

The CLI is the product. The TUI is one consumer. So is the agent skill. So is anything you build on top.

## Why

Debuggers are the last terminal-first tool that hasn't been wrapped by a `lazy*` interface, and the only one that matters for AI agents that need to inspect runtime state. Existing options are MCP-tied, VS Code-tied, or DAP-tied (assume you already speak DAP). Nothing exposes debugging as plain shell subcommands an agent can invoke via Bash. lazydap fills that gap.

See [docs/blueprint/00-overview.md](docs/blueprint/00-overview.md) for the full thesis.

## Quick read

| You want | Read |
|---|---|
| Why this project exists | [`docs/blueprint/00-overview.md`](docs/blueprint/00-overview.md) |
| How it's structured | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| Where the work is going | [`docs/implementation/`](docs/implementation/) |
| What's coming next | [`TODO.md`](TODO.md) |
| Decisions and why | [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md) |
| Existing competitors | [`docs/articles/agent-driven-debugging.md`](docs/articles/agent-driven-debugging.md) |
| "Isn't this just a wrapper on DAP?" | [`docs/articles/yes-its-a-wrapper.md`](docs/articles/yes-its-a-wrapper.md) |
| For agents (Claude/Cursor/etc) | [`AGENTS.md`](AGENTS.md) |
| For Claude Code specifically | [`CLAUDE.md`](CLAUDE.md) |
| Setting up a dev environment | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

## Install (as a user)

Not yet. M0 is "spawn an adapter, read its output." Public release is targeted around milestone [M15](docs/implementation/tasks/M15-config-file.md). When it ships, this section will cover installing the `lazydap` binary and pointing it at a DAP adapter on your system.

## Develop (working on lazydap itself)

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for toolchain prereqs, build / test / lint commands, and how to install the DAP adapters (codelldb, debugpy, js-debug) that the integration tests run against.

## License

Dual-licensed under MIT OR Apache-2.0, matching Rust ecosystem convention. See `LICENSE-MIT` and `LICENSE-APACHE` (TODO: add when v0.1 ships).
