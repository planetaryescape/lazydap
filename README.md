# lazydap

Debug a C, C++ or Rust program from the shell. Set a breakpoint with `lazydap break hello.c:6`, run to it with `lazydap continue --wait`, read the JSON that comes back, decide what to do next. A daemon holds the debug session so each command can be a separate process; a terminal UI ships in the same binary, as a client of the same socket the CLI talks to.

> **Pre-release.** `v0.1.0` is not tagged and there are no published binaries. Install from source, as below. Every command on this page was run against this commit and every reply is the one lazydap gave, reflowed and elided where it was long. [What isn't built yet](#what-works-today) is listed near the bottom.

## The loop

```console
$ lazydap break hello.c:6 --format json
{
  "action": "added",
  "applied_to_session": false,
  "breakpoints": [
    { "enabled": true, "id": 1, "line": 6,
      "source": "/Users/you/lazydap-demo/hello.c", "verified": false }
  ],
  "dry_run": false,
  "not_found": []
}

$ lazydap launch ./hello --format json
{
  "capabilities": { "supports_conditional_breakpoints": true, ... },
  "session_id": "8a2b018f-ecbf-4602-aaf3-17a022dc1220",
  "state": "running",
  ...
}

$ lazydap continue --wait --format json
{
  "captured_output": [
    { "category": "stdout", "output": "starting\r\n", "timestamp_ms": 1785443685097 },
    ...
  ],
  "elapsed_ms": 90,
  "frame": {
    "column": 16, "id": 1001, "line": 6, "name": "total",
    "source": { "name": "hello.c", "path": "/Users/you/lazydap-demo/hello.c" }
  },
  "hit_breakpoint_ids": [1],
  "reason": "breakpoint",
  "state": "paused",
  "thread_id": 27619421,
  ...
}
```

`--wait` is the flag that makes this work from a shell. Without it a step command returns the moment the debugger accepts the request, before the program has gone anywhere. With it, lazydap blocks until the program is somewhere worth looking at and returns one object describing the whole trip: where it stopped, why, the top frame, which breakpoints it hit, and every line the program printed on the way. That last part is usually what you were after.

Then look around, while it's paused:

```console
$ lazydap variables --reference 1003 --format json
{
  "variables": [
    { "name": "n",   "type_name": "int", "value": "10", "variables_reference": 0 },
    { "name": "sum", "type_name": "int", "value": "0",  "variables_reference": 0 },
    { "name": "i",   "type_name": "int", "value": "1",  "variables_reference": 0 }
  ]
}

$ lazydap eval 'n * 2' --format json
{ "type_name": "long long", "value": "20", "variables_reference": 0 }

$ lazydap step --wait --format json
{
  "frame": { "column": 5, "id": 1007, "line": 7, "name": "total", ... },
  "reason": "step",
  "state": "paused",
  ...
}
```

## What it isn't

Five deliberate trade-offs. Each one has a defensible opposite, and plenty of tools take it.

**Shell subcommands, not an MCP server.** Anything that can run a command can drive lazydap: a CI job, a Makefile, a vim autocommand, an agent with a Bash tool and nothing else. The cost is that you get no tool discovery and no typed schema handed to your model by a host — an MCP-native debugger gives you that, and every other project in this niche is MCP-first for exactly that reason. An MCP bridge over this protocol is a small separate crate, and it isn't in this repo.

**One blob per command, not an event stream.** A stepping command returns after the program settles, with the intervening events folded into the reply. A tool built around a live event feed would show you output the instant it appears, which is better for a dashboard and worse for a script that has to decide what to do next. If you want the feed, a long-lived client can `Subscribe` on the socket — that is what the TUI does.

**The JSON is the contract. The table output is not.** `--format json` has a stable schema and breaking it costs a decision-log entry. `--format table` is for your eyes and will be reflowed whenever it reads badly. Tools that ship one pretty output and tell you to parse it are making the opposite bet, and it works fine right up until the day it doesn't.

**It wraps codelldb rather than being a debugger.** DWARF parsing, ptrace, expression evaluation in the debuggee's language: LLDB already does all of that, better than a rewrite would. The bill comes as codelldb's quirks becoming yours — [seven of them are written down](docs/reference/codelldb-quirks.md) — and as bugs lazydap cannot fix because they live a layer down.

**One debug session per project at a time.** The daemon is per project root and holds one session. Launching a second while the first is live gets you `SessionAlreadyActive`. Multi-session is designed for (session ids are in the protocol from the start) and not built. If you want to debug four services at once today, this is the wrong tool.

## Install

**Rust toolchain.** [rustup](https://rustup.rs/); the pinned channel comes from `rust-toolchain.toml` on first `cargo` invocation in the repo.

**codelldb**, on `PATH`. Grab the `.vsix` for your platform from the [latest release](https://github.com/vadimcn/codelldb/releases/latest) (a `.vsix` is a renamed zip; VS Code is not involved):

```bash
curl -sL -o /tmp/codelldb.vsix \
  https://github.com/vadimcn/codelldb/releases/latest/download/codelldb-darwin-arm64.vsix
mkdir -p ~/.local/opt/codelldb
unzip -q -o /tmp/codelldb.vsix -d ~/.local/opt/codelldb

# A wrapper script, not a symlink.
cat > ~/.local/bin/codelldb <<'EOF'
#!/usr/bin/env bash
exec "$HOME/.local/opt/codelldb/extension/adapter/codelldb" "$@"
EOF
chmod +x ~/.local/bin/codelldb

codelldb --help      # --version is not a flag it knows
```

> `ln -s` will not work here. codelldb locates `liblldb` by walking up from `argv[0]`, so a symlink on `PATH` sends it looking one directory too high and it dies in `dlopen`. The wrapper hands it an absolute path. Full write-up, plus the macOS-update failure that makes every invocation hang at `_dyld_start`: [`docs/reference/codelldb-quirks.md`](docs/reference/codelldb-quirks.md).

Other platforms: `codelldb-linux-x64.vsix`, `codelldb-linux-arm64.vsix`, `codelldb-darwin-x64.vsix`.

**lazydap itself**, from source:

```bash
git clone https://github.com/planetaryescape/lazydap
cd lazydap
cargo install --path crates/daemon      # installs one binary: lazydap
```

Then check the pieces are where lazydap expects:

```console
$ lazydap doctor --format json
{
  "checks": [
    { "detail": "/Users/you/.local/bin/codelldb", "name": "adapter.codelldb", "ok": true },
    { "detail": "/Users/you/lazydap-demo/.lazydap/state.toml (0 breakpoints)", "name": "state.file", "ok": true },
    { "detail": "instance lazydap-demo-13cc8efcde46, pid 43293, protocol v2", "name": "daemon", "ok": true }
  ],
  "ok": true
}
```

`doctor` starts the daemon if one isn't running, which is also true of every other command — there is nothing to start by hand.

## Quickstart

```bash
mkdir -p ~/lazydap-demo && cd ~/lazydap-demo
cat > hello.c <<'EOF'
#include <stdio.h>

int total(int n) {
    int sum = 0;
    for (int i = 1; i <= n; i++) {
        sum += i;
    }
    return sum;
}

int main(void) {
    printf("starting\n");
    fflush(stdout);
    int answer = total(10);
    printf("total=%d\n", answer);
    return 0;
}
EOF
gcc -g -O0 hello.c -o hello
```

`-g` is required (no symbols, no source lines) and `-O0` keeps the line numbers honest.

```bash
lazydap break hello.c:6           # inside the loop
lazydap launch ./hello
lazydap continue --wait           # runs to the breakpoint
lazydap scopes                    # note the Local scope's variables_reference
lazydap variables --reference 1003
lazydap eval 'sum + i'
lazydap continue --wait           # next iteration
lazydap disconnect                # end the session, keep the daemon
```

Two things that will otherwise cost you a turn:

- **A `variables_reference` is only valid until the program next moves.** Re-run `scopes` after each stop; a stale reference gets you `DapProtocolError: Invalid variabes reference` (the typo is codelldb's).
- **Breakpoints are project state.** They live in `.lazydap/state.toml`, survive both the session and the daemon, and are re-applied to every later launch. `lazydap break --list` shows them, `lazydap break --all --remove` clears them.

Errors are JSON on stderr, with the exit code as the real signal (`0` fine, `1` operation failed, `2` bad usage, `3` no daemon, `4` no adapter):

```console
$ lazydap variables --ref 1005 --format json
{"details":{"kind":"UnknownArgument"},"error":"UsageError","message":"error: unexpected argument '--ref' found\n\n  tip: a similar argument exists: '--reference'\n..."}
$ echo $?
2
```

## The TUI

Bare `lazydap` on a terminal opens it; `lazydap tui` is the explicit spelling. In a pipe or a CI job the same command prints help instead, because the tty check covers stdin and stdout.

It shows the source file with a marker on the current line, and drives the program with the same requests the CLI sends: `F5`/`c` continue, `F10`/`n` step over, `F11` step in, `Shift-F11` step out. `j`/`k`/`<C-d>`/`<C-u>`/`gg`/`G` move the view. `q` leaves without ending the session, so you can walk out of the TUI and keep going from the shell against the same paused program.

The TUI cannot reach the daemon's internals, and this is enforced by the dependency graph rather than by discipline: `lazydap-tui` depends on `lazydap-core` and `lazydap-protocol` and nothing else, so a feature that skips the protocol does not compile. Stack, scopes and breakpoint panes are the next milestones and are not there yet.

## For agents

`lazydap.skill` at the repository root is a zip containing `SKILL.md` plus a generated command reference, output schemas, error codes, and worked sessions. Point an agent at it and it drives the debugger the way you would.

The reference is generated from lazydap's own argument parser and CI fails if the committed artifact drifts from its sources, so an agent reading it never sees a flag that no longer exists.

For agents working *on* this repo rather than with it, [`AGENTS.md`](AGENTS.md) is the entry point.

## What works today

Launch, breakpoints (set, list, remove, toggle, conditional), continue, step over, step in, step out, pause, stack, scopes, variables, eval, threads, captured output, status, disconnect, shutdown, doctor, version, logs, completions. `--wait` and `--timeout` on everything that moves the program. Output as `table`, `json`, `jsonl`, `csv` or `ids`, chosen automatically from the tty and overridable with `--format`. Persistent breakpoints. A daemon per project with a live event stream clients can subscribe to. A source-pane TUI.

Scope for v0.1: **codelldb only** (C, C++, Rust), **one session at a time**, **macOS and Linux**.

Not built, in rough order: TUI panes for stack, scopes and breakpoints; a config file and `.vscode/launch.json` import; watches; a REPL pane; `attach`; restart. After that, debugpy for Python, then delve and js-debug, then multi-session. [`TODO.md`](TODO.md) is the live list and [`docs/blueprint/14-roadmap.md`](docs/blueprint/14-roadmap.md) is the plan behind it.

Windows is not a target. Nothing here phones home ([`PRIVACY.md`](PRIVACY.md)).

## Architecture

Clients speak length-delimited JSON over a Unix socket to a daemon that owns the DAP adapter, and the crate graph makes it impossible for a client to go around them.

[`ARCHITECTURE.md`](ARCHITECTURE.md) has the shape and the rules; [`docs/blueprint/01-architecture.md`](docs/blueprint/01-architecture.md) expands it; [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md) records why each call went the way it did.

## Docs

| | |
|---|---|
| Why this exists | [`docs/blueprint/00-overview.md`](docs/blueprint/00-overview.md) |
| "Isn't this just a wrapper on DAP?" | [`docs/articles/yes-its-a-wrapper.md`](docs/articles/yes-its-a-wrapper.md) |
| Why the CLI and not the TUI is the product | [`docs/articles/the-cli-is-the-product.md`](docs/articles/the-cli-is-the-product.md) |
| What else exists in this space | [`docs/articles/agent-driven-debugging.md`](docs/articles/agent-driven-debugging.md) |
| How `--wait` is specified | [`docs/blueprint/10-async-to-sync.md`](docs/blueprint/10-async-to-sync.md) |
| codelldb's quirks, forensically | [`docs/reference/codelldb-quirks.md`](docs/reference/codelldb-quirks.md) |
| How debuggers work at all | [`docs/reference/how-debuggers-actually-work.md`](docs/reference/how-debuggers-actually-work.md) |
| Setting up to hack on it | [`CONTRIBUTING.md`](CONTRIBUTING.md) |
| What changed | [`CHANGELOG.md`](CHANGELOG.md) |
| Reporting a vulnerability | [`SECURITY.md`](SECURITY.md) |

lazydap is also the subject of a learn-by-LLM Rust book. The chapters under [`docs/book/`](docs/book/) are a snapshot; they are owned by `lazydap-learn`, a separate and currently private repository.

## License

MIT or Apache-2.0, your choice. See [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).
