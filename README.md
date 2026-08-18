# lazydap

Debug from the shell: one command per step, one JSON answer per command — for your agent, your scripts, and your CI.

lazydap turns a real debugger into shell subcommands that answer in JSON you can build on. Set a breakpoint with `lazydap break hello.c:6`, run to it with `lazydap continue --wait`, read one object that says where the program stopped, why, and everything it printed on the way. A daemon holds the session so each command can be a separate process; an agent drives it with nothing but a Bash tool ([the skill ships in this repo](#for-agents)); CI branches on its exit codes. The schema and the exit codes are contracts — kept stable on purpose and tested against real codelldb, debugpy and delve.

It exists for the work where a debugger is not optional: memory you manage by hand, crashes that destroy their own evidence, races you cannot printf around, and the native library underneath your Python.

> **Early.** `v0.2.5` is a prerelease. Every command on this page was run against this commit and every reply is the one lazydap gave, reflowed and elided where it was long. [What isn't built yet](#what-works-today) is listed near the bottom.

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

$ lazydap launch ./hello --stop-on-entry --format json
{
  "capabilities": { "supports_conditional_breakpoints": true, ... },
  "raw_reason": "exception",
  "reason": "entry",
  "session_id": "64612148-bae7-44d3-a3cd-18f87f2c82b4",
  "state": "paused",
  "thread_id": 27790565,
  ...
}

$ lazydap continue --wait --format json
{
  "captured_output": [
    { "category": "stdout", "output": "starting\r\n", "timestamp_ms": 1785444921957 },
    ...
  ],
  "elapsed_ms": 98,
  "frame": {
    "column": 16, "id": 1001, "line": 6, "name": "total",
    "source": { "name": "hello.c", "path": "/Users/you/lazydap-demo/hello.c" }
  },
  "hit_breakpoint_ids": [1],
  "reason": "breakpoint",
  "state": "paused",
  "thread_id": 27790565,
  ...
}
```

`--stop-on-entry` is doing real work there. Without it the program starts running the moment `launch` returns, and if it reaches your breakpoint before the `continue` command gets there, that `continue` resumes from the stop you wanted rather than running to it. Stopping at entry puts you in control of when the program first moves.

The pair of reason fields is lazydap declining to tell you a tidy lie. codelldb implements entry-stop by sending the process a `SIGSTOP`, which LLDB classifies as an exception, so the adapter says `exception`. `reason` is the normalised answer and `raw_reason` is what the adapter actually said, because a reader who needs to know the difference should not have to find out by experiment.

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

Six deliberate trade-offs. Each one has a defensible opposite, and a real tool takes it.

**The CLI is the product, not the plumbing under a skill.** Several 2026 tools wrap a debugger daemon for agents and lead with the agent skill — the binary underneath is undocumented infrastructure. lazydap inverts that: the same binary, with the same documented schema and exit codes, serves an agent's Bash tool, a shell script, a Makefile, and a CI assertion. Pipe it into `jq`, branch on exit code `4`, clear every breakpoint with `--format ids | xargs`. The cost is that the agent gets no curated abstraction beyond the skill's advice — an agent-only surface can be simpler because nothing else has to compose with it.

**Shell subcommands, not an MCP server.** Anything that can run a command can drive lazydap: a CI job, a Makefile, a vim autocommand, an agent with a Bash tool and nothing else. The cost is that you get no tool discovery and no typed schema handed to your model by a host — an MCP-native debugger gives you that, and every other project in this niche is MCP-first for exactly that reason. An MCP bridge over this protocol is a small separate crate, and it isn't in this repo.

**One blob per command, not an event stream.** A stepping command returns after the program settles, with the intervening events folded into the reply. A tool built around a live event feed would show you output the instant it appears, which is better for a dashboard and worse for a script that has to decide what to do next. If you want the feed, a long-lived client can `Subscribe` on the socket — that is what the TUI does.

**The JSON is the contract. The table output is not.** `--format json` has a stable schema and breaking it costs a decision-log entry. `--format table` is for your eyes and will be reflowed whenever it reads badly. Tools that ship one pretty output and tell you to parse it are making the opposite bet, and it works fine right up until the day it doesn't.

**It wraps codelldb rather than being a debugger.** DWARF parsing, ptrace, expression evaluation in the debuggee's language: LLDB already does all of that, better than a rewrite would. The bill comes as codelldb's quirks becoming yours — [twenty-six of them are written down](docs/reference/codelldb-quirks.md) — and as bugs lazydap cannot fix because they live a layer down.

**One debug session per project at a time.** The daemon is per project root and holds one session. Launching a second while the first is live gets you `SessionAlreadyActive`. Multi-session is designed for (session ids are in the protocol from the start) and not built. If you want to debug four services at once today, this is the wrong tool.

## Install

**codelldb**, on `PATH`. Grab the `.vsix` for your platform from the [latest release](https://github.com/vadimcn/codelldb/releases/latest) (a `.vsix` is a renamed zip; VS Code is not involved):

```bash
curl -sL -o /tmp/codelldb.vsix \
  https://github.com/vadimcn/codelldb/releases/latest/download/codelldb-darwin-arm64.vsix
mkdir -p ~/.local/opt/codelldb
unzip -q -o /tmp/codelldb.vsix -d ~/.local/opt/codelldb

# A wrapper script, not a symlink. ~/.local/bin must exist and be on your PATH.
mkdir -p ~/.local/bin
cat > ~/.local/bin/codelldb <<'EOF'
#!/usr/bin/env bash
exec "$HOME/.local/opt/codelldb/extension/adapter/codelldb" "$@"
EOF
chmod +x ~/.local/bin/codelldb

codelldb --help      # --version is not a flag it knows
```

> `ln -s` will not work here. codelldb locates `liblldb` by walking up from `argv[0]`, so a symlink on `PATH` sends it looking one directory too high and it dies in `dlopen`. The wrapper hands it an absolute path. Full write-up, plus the macOS-update failure that makes every invocation hang at `_dyld_start`: [`docs/reference/codelldb-quirks.md`](docs/reference/codelldb-quirks.md).

Other platforms: `codelldb-linux-x64.vsix`, `codelldb-linux-arm64.vsix`, `codelldb-darwin-x64.vsix`.

**lazydap itself**, by Homebrew:

```bash
brew install planetaryescape/lazydap/lazydap
```

Or by script, which reads `uname`, downloads the matching build, checks its SHA-256 **before** unpacking it, and puts the binary in `~/.local/bin` — `LAZYDAP_INSTALL_DIR` moves that, and nothing here uses `sudo`:

```bash
curl -fsSL https://raw.githubusercontent.com/planetaryescape/lazydap/main/install.sh | bash
```

Or from source, which is the route on any platform outside the released builds — those cover macOS arm64 and x86_64 and Linux x86_64. Needs a [rustup](https://rustup.rs/) toolchain; the pinned channel comes from `rust-toolchain.toml` on the first `cargo` invocation in the repo:

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
    { "detail": "none; create /Users/you/.config/lazydap/config.toml to add one", "name": "config.file", "ok": true },
    { "detail": "/Users/you/.local/bin/codelldb", "name": "adapter.codelldb", "ok": true },
    { "detail": "/opt/homebrew/bin/python3", "name": "adapter.debugpy", "ok": true },
    { "detail": "no delve binary found on PATH — install it with `go install github.com/go-delve/delve/cmd/dlv@latest`", "name": "adapter.delve", "ok": false },
    { "detail": "/Users/you/lazydap-demo/.lazydap/state.toml (0 breakpoints)", "name": "state.file", "ok": true },
    { "detail": "instance lazydap-demo-13cc8efcde46, pid 43293, protocol v9", "name": "daemon", "ok": true }
  ],
  "ok": true
}
```

`ok` means lazydap can debug something here — not that this machine has every adapter lazydap ships. The missing delve above is reported and costs you nothing until you want to debug Go; what has to be sound is the config file, the state file, the daemon, and at least one adapter.

`doctor` starts the daemon if one isn't running, which is also true of every other command — there is nothing to start by hand. The exception is `doctor --check-state`, which reads `.lazydap/state.toml` itself and starts nothing — so it can name the broken line in a state file that a daemon refuses to start on.

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
lazydap launch ./hello --stop-on-entry
lazydap continue --wait           # runs to the breakpoint
lazydap scopes                    # note the Local scope's variables_reference
lazydap variables --reference 1003
lazydap eval 'sum + i'
lazydap continue --wait           # next iteration
lazydap disconnect                # end the session, keep the daemon
```

Two things that will otherwise cost you a turn:

- **A `variables_reference` is only valid until the program next moves.** Re-run `scopes` after each stop; a stale one is refused with `StaleHandle` before the debugger is asked anything. The handles are lazydap's, minted per stop and never reused, so an old number can never come back full of another stop's variables.
- **Breakpoints are project state.** They live in `.lazydap/state.toml`, survive both the session and the daemon, and are re-applied to every later launch. `lazydap break --list` shows them, `lazydap break --all --remove` clears them.

Errors are JSON on stderr, with the exit code as the real signal (`0` fine, `1` operation failed, `2` bad usage, `3` no daemon, `4` no adapter):

```console
$ lazydap variables --ref 1005 --format json
{"details":{"kind":"UnknownArgument"},"error":"UsageError","message":"error: unexpected argument '--ref' found\n\n  tip: a similar argument exists: '--reference'\n..."}
$ echo $?
2
```

## Configurations the repository already has

Most repositories with a non-trivial debug setup already carry a `.vscode/launch.json`. lazydap reads it — and never writes it — alongside its own `[[launch_configs]]` in `.lazydap/state.toml`:

```console
$ lazydap launches list
NAME          SOURCE       ADAPTER  REQUEST  PROGRAM                          RUNNABLE
Debug binary  launch.json  lldb     launch   /Users/you/demo/build/demo       yes
API           launch.json  python   launch   /Users/you/demo/app.py           yes
Pick one      launch.json  lldb     launch   /Users/you/demo/${command:pickProcess}  no (nothing could expand ${command:pickProcess}, so its paths are not paths)

warning: `Pick one` uses ${command:pickProcess}, which nothing here can expand; it is left as written

$ lazydap launches run "Debug binary" --stop-on-entry --format json
{"session_id":"7f866ac8-...","state":"paused","reason":"entry", ...}
```

VS Code's dialect is read as written: `//` and `/* */` comments and trailing commas. `${workspaceFolder}`, `${workspaceFolderBasename}`, `${userHome}` and `${env:VAR}` are expanded.

**A variable nothing can expand is left in the string and reported, not quietly replaced with nothing.** VS Code substitutes the empty string, which turns `${env:BUILD_DIR}/app` into `/app` — a real path on every Unix machine and the wrong one on all of them. lazydap marks the configuration unrunnable and says which variable it was. Configurations for other debuggers, and `attach` configurations, are listed for the same reason: telling you your file has four configurations and lazydap can run one of them beats showing one and dropping three.

## Configuring lazydap itself

Optional. Without a config file lazydap runs on its defaults and writes nothing. With one — at `~/.config/lazydap/config.toml`, at `$XDG_CONFIG_HOME/lazydap/config.toml`, or wherever `LAZYDAP_CONFIG_PATH` points — two settings are read:

```toml
[general]
wait_timeout_seconds = 45          # the default for --wait, under --timeout and LAZYDAP_TIMEOUT

[adapter.codelldb]
command = "/opt/codelldb/codelldb" # pin the adapter, ahead of PATH
```

A pinned adapter that isn't there is an error naming the path, not a quiet fall-through to whatever is on `PATH` — using a different build of the adapter than the one you chose, and not saying so, is worse than failing. Keys this build does not read yet are ignored rather than rejected, so a file written against the fuller schema in [`docs/blueprint/08-state-and-config.md`](docs/blueprint/08-state-and-config.md) keeps working as those land.

## The TUI

Bare `lazydap` on a terminal opens it; `lazydap tui` is the explicit spelling. In a pipe or a CI job the same command prints help instead, because the tty check covers stdin and stdout.

Three panes — the source file with a marker on the current line, the call stack, and the variable scopes of the selected frame. `Tab` moves between them, `<CR>` jumps to a frame or expands a variable, `b` sets or clears a breakpoint on the cursor line. It drives the program with the same requests the CLI sends: `F5`/`c` continue, `F10`/`n` step over, `F11` step in, `Shift-F11` step out. `j`/`k`/`<C-d>`/`<C-u>`/`gg`/`G` move the view. `q` leaves without ending the session, so you can walk out of the TUI and keep going from the shell against the same paused program. If the daemon goes away underneath it, the TUI says so and reconnects on its own, starting one if there is none.

The TUI cannot reach the daemon's internals, and this is enforced by the dependency graph rather than by discipline: `lazydap-tui` depends on `lazydap-core`, `lazydap-config` and `lazydap-protocol` and nothing else, so a feature that skips the protocol does not compile. That is also why every one of those keys has a CLI equivalent: a TUI-only feature is not something that can be written.

## For agents

`lazydap.skill` at the repository root is a zip containing `SKILL.md` plus a generated command reference, output schemas, error codes, and worked sessions. Point an agent at it and it drives the debugger the way you would.

The reference is generated from lazydap's own argument parser and CI fails if the committed artifact drifts from its sources, so an agent reading it never sees a flag that no longer exists.

For agents working *on* this repo rather than with it, [`AGENTS.md`](AGENTS.md) is the entry point.

## What works today

Launch, breakpoints (set, list, remove, toggle, conditions, hit counts, log points), continue, step over, step in, step out, pause, stack, scopes, variables, eval, threads, watches, captured output, launches, status, disconnect, shutdown, doctor, version, logs, completions. `--wait` and `--timeout` on everything that moves the program. Output as `table`, `json`, `jsonl`, `csv` or `ids`, chosen automatically from the tty and overridable with `--format`. Persistent breakpoints and watches. A config file for adapter pins and the default timeout, and `.vscode/launch.json` import. A daemon per project with a live event stream clients can subscribe to. A TUI with source, stack, scopes, watches and REPL panes that reconnects on its own.

Scope today: **C, C++ and Rust via codelldb, Python via debugpy, Go via delve**, **one session at a time**, **macOS and Linux**. (All of it ships in the v0.2.0 release; v0.1.0 was codelldb-only.)

Not built, in rough order: `attach`; restart; conditional breakpoints from the TUI; the rest of the config schema. After that, js-debug for Node, then multi-session. [`TODO.md`](TODO.md) is the live list and [`docs/blueprint/14-roadmap.md`](docs/blueprint/14-roadmap.md) is the plan behind it.

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
| State and config file schemas | [`docs/blueprint/08-state-and-config.md`](docs/blueprint/08-state-and-config.md) |
| Setting up to hack on it | [`CONTRIBUTING.md`](CONTRIBUTING.md) |
| What changed | [`CHANGELOG.md`](CHANGELOG.md) |
| Reporting a vulnerability | [`SECURITY.md`](SECURITY.md) |

The documentation website's sources are in [`site/`](site/).

lazydap is also the subject of a learn-by-LLM Rust book. The chapters under [`docs/book/`](docs/book/) are a snapshot; they are owned by `lazydap-learn`, a separate and currently private repository.

## License

MIT or Apache-2.0, your choice. See [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).
