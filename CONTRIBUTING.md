# Contributing to lazydap

How to get a working dev environment, what has to be green before you push, and the handful of rules that aren't negotiable. Architecture lives in [`ARCHITECTURE.md`](ARCHITECTURE.md); the working conventions agents follow are in [`AGENTS.md`](AGENTS.md).

## Prerequisites

- **Rust toolchain** — pinned in [`rust-toolchain.toml`](rust-toolchain.toml) (stable, with `rustfmt` and `clippy`). Install [rustup](https://rustup.rs/) and `cargo` picks the right version on its first run in this directory. The workspace is edition 2024 with `rust-version = "1.85"`.
- **macOS or Linux.** Windows is not a target.
- **codelldb on `PATH`.** Required for the integration suite; see below. The tests skip loudly without it rather than failing, so you can work on protocol or TUI code without one, but don't claim a green run you didn't get.
- **A C compiler.** `gcc` or `clang`. The integration tests compile their own fixtures at runtime and skip when they can't.

## Build, test, lint

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo check --workspace --all-targets
cargo test --workspace --all-targets
```

Two more gates that CI runs and `cargo` doesn't:

```bash
bash scripts/check_architecture_boundaries.sh     # crate graph matches the rules
bash scripts/build-skill.sh && git diff --exit-code -- skill/ lazydap.skill
```

The boundary script reads `Cargo.toml` files and fails if a crate grew a dependency it isn't allowed. It doesn't compile anything, so it's cheap enough to run constantly.

The skill check rebuilds `lazydap.skill` and its generated command reference from `skill/`, then diffs. The build is byte-for-byte reproducible (fixed timestamps, sorted entries), so a clean diff proves the committed artifact matches its sources rather than merely that somebody rebuilt it. Add a CLI flag and forget this step and CI will tell you, because an agent reading a stale reference is a bug you can't see.

CI additionally sets `RUSTFLAGS: -Dwarnings`; locally you'll want the same before you push.

## Test layout

| Where | What it is |
|---|---|
| `crates/*/src/**` unit tests | Anything decidable without a real process. The `--wait` event arithmetic (watermarks, coalescing, output caps) is unit-tested deterministically in `crates/daemon/src/wait.rs`. |
| `crates/daemon/tests/ipc_server.rs` | The socket contract, driven in-process. |
| `crates/daemon/tests/cli_lifecycle.rs` | Daemon auto-spawn, staying up, and shutting down, through the real binary. Each test gets its own instance name plus its own `LAZYDAP_RUNTIME_DIR` and `LAZYDAP_DATA_DIR`, so it never touches your actual daemon. No debuggee is launched. |
| `crates/daemon/tests/wait_codelldb.rs` | `--wait` against real codelldb and real debuggees. The case list comes from [`docs/blueprint/10-async-to-sync.md`](docs/blueprint/10-async-to-sync.md). |
| `crates/daemon/tests/wait_debugpy.rs`, `wait_delve.rs` | The same cases against debugpy and delve. Where an assertion differs from its codelldb twin, that difference is the finding and it is commented. |

**Why `wait_codelldb.rs` serialises itself.** Every test there spawns a codelldb, which loads LLDB, maps a debuggee and opens a TCP socket. `cargo test` runs a file's tests in parallel, so a dozen of those start at once and fight over the machine — and the launch handshake has a 15-second deadline. Under that contention the deadline lands before the adapter is ready, and the suite fails for reasons that have nothing to do with lazydap: 12 of 13 timed out on a reviewer's machine while every one passed in isolation. A file-level mutex makes them take turns. It costs a few seconds of wall clock and buys a suite whose failures mean something. If you add a test there, take the same lock.

**Making a skip a failure.** The three `wait_*.rs` suites skip themselves when their adapter is missing, and a skip is the same colour as a pass. Set `LAZYDAP_REQUIRE_ADAPTERS=1` and a missing adapter fails the test instead, naming what it wanted:

```bash
LAZYDAP_REQUIRE_ADAPTERS=1 cargo test -p lazydap-daemon \
  --test wait_codelldb --test wait_debugpy --test wait_delve
```

Empty and `0` count as unset, so `LAZYDAP_REQUIRE_ADAPTERS= cargo test` is how you switch it back off in a shell that has exported it. CI's `adapters` job installs codelldb, debugpy and dlv and runs exactly that, so "the canonical tests run real codelldb" is now checked rather than trusted. The plain `test` job installs nothing, which keeps the skip path honest. Use the variable locally when you want to know that your run really exercised an adapter.

**What not to mock.** The daemon, the store, and the `DebugAdapter` trait are lazydap's own. Mocking them tests the mock. There is no `FakeAdapter`: where the thing under test genuinely isn't adapter behaviour, use `AdapterHandle::detached()` — `#[cfg(test)] pub(crate)`, every request answers `Gone`, and reachable only from a unit test inside `crates/daemon/src/`, not from `crates/daemon/tests/` — or a scripted transport as `crates/dap/src/transport.rs` does. Anything that is a claim about what an adapter does belongs in `wait_codelldb.rs`, `wait_debugpy.rs` or `wait_delve.rs` against the real one.

## Sanity-checking a change by hand

A quick end-to-end loop from the workspace root, using the fixture in this repo:

```bash
mkdir -p examples/c-hello/build          # gitignored, so absent in a fresh clone
gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello
./target/debug/lazydap break examples/c-hello/main.c:19
./target/debug/lazydap launch ./examples/c-hello/build/hello --stop-on-entry
./target/debug/lazydap continue --wait
./target/debug/lazydap stack
./target/debug/lazydap shutdown
```

> **Don't put a debuggee under `/tmp` on macOS.** `/tmp` is a symlink to `/private/tmp`. lazydap canonicalises the source path and the compiler didn't, so codelldb gets asked about a file under a name it doesn't recognise and your breakpoint silently fails to bind: `verified: false`, a `message` reading `"could not be resolved, but a valid location was found at /tmp/..."`, and a program that runs straight through. Use a directory under `$HOME`. [Quirk 8](docs/reference/codelldb-quirks.md#8-breakpoints-never-bind-for-a-debuggee-under-tmp-on-macos).

`lazydap logs` shows the daemon's log; `LAZYDAP_LOG` takes `tracing` filter directives (`LAZYDAP_LOG=dap.recv.event=debug` to watch raw adapter events), falling back to `RUST_LOG`. `lazydap daemon --foreground` keeps it in your terminal instead.

## DAP adapters

lazydap wraps external DAP adapter processes. codelldb, debugpy and delve are wired up; js-debug is on the roadmap, and its install notes are here because you'll want them when that lands. `cargo test --workspace` skips the suite for any adapter this machine does not have, so install the ones whose behaviour you are changing.

| Adapter | Languages | Source |
|---|---|---|
| **codelldb** | Rust, C, C++, Swift, anything LLDB can debug | [vadimcn/codelldb](https://github.com/vadimcn/codelldb) |
| **debugpy** | Python | [microsoft/debugpy](https://github.com/microsoft/debugpy) (PyPI) |
| **delve** | Go | [go-delve/delve](https://github.com/go-delve/delve) |
| **js-debug** | Node.js, Chrome, Edge — *not wired up* | [microsoft/vscode-js-debug](https://github.com/microsoft/vscode-js-debug) |

The convention below: third-party prebuilt blobs go in `~/.local/opt/<name>/`, executables get exposed on `PATH` via `~/.local/bin/`. Make sure `~/.local/bin` is on your `PATH`. Nothing in lazydap depends on these locations.

### codelldb (C / C++ / Rust)

Upstream ships platform-specific `.vsix` bundles containing the prebuilt `codelldb` binary and its `liblldb`. A `.vsix` is a renamed zip; VS Code is not required.

```bash
# 1. Download the bundle for your platform.
curl -sL -o /tmp/codelldb.vsix \
  https://github.com/vadimcn/codelldb/releases/latest/download/codelldb-darwin-arm64.vsix

# 2. Extract.
mkdir -p ~/.local/opt/codelldb
unzip -q -o /tmp/codelldb.vsix -d ~/.local/opt/codelldb
#    binary  → ~/.local/opt/codelldb/extension/adapter/codelldb
#    liblldb → ~/.local/opt/codelldb/extension/lldb/lib/liblldb.dylib

# 3. Expose on PATH with a wrapper script. NOT a symlink.
mkdir -p ~/.local/bin
cat > ~/.local/bin/codelldb <<'WRAPPER_EOF'
#!/usr/bin/env bash
exec "$HOME/.local/opt/codelldb/extension/adapter/codelldb" "$@"
WRAPPER_EOF
chmod +x ~/.local/bin/codelldb

# 4. Verify.
codelldb --help            # --version is not a flag it recognises
```

Other platforms: `codelldb-linux-x64.vsix`, `codelldb-linux-arm64.vsix`, `codelldb-darwin-x64.vsix`.

Two failure modes worth knowing before you lose an afternoon to either:

> **A symlink on `PATH` breaks it.** codelldb finds `liblldb` by walking up from `argv[0]`, so through a symlink at `~/.local/bin/codelldb` it looks one directory too high and panics in `dlopen`. The wrapper above hands it an absolute path. [Quirk 1](docs/reference/codelldb-quirks.md#1-symlink-install-breaks-liblldb-resolution).

> **After a macOS update, every invocation may hang at `_dyld_start`** — including `codelldb --help` — with no output at all, because the OS is holding a stale per-inode security record. Copying the install to fresh inodes fixes it: `rm -rf ~/.local/opt/codelldb`, re-extract, re-verify. [Quirk 5](docs/reference/codelldb-quirks.md#5-hangs-at-_dyld_start-after-a-macos-update-stale-gatekeeper-inode-cache).

> **Don't `code --install-extension vadimcn.vscode-lldb` and expect a CLI binary.** That marketplace package lazy-downloads the platform binary on first activation inside VS Code. Fine in the IDE, useless standalone.

Every codelldb behaviour that has cost this project time is written up in [`docs/reference/codelldb-quirks.md`](docs/reference/codelldb-quirks.md). Read it before filing a bug against lazydap for something the adapter did.

### debugpy (Python)

lazydap runs debugpy as `<python> -m debugpy.adapter` over the child's stdin and stdout, so
what has to be installed is the *module*, in the interpreter `python3` resolves to on your
`PATH` — not the `debugpy-adapter` shim. A `pipx install debugpy` puts the shim on `PATH`
and the module in an isolated environment `python3` cannot import, which passes a `--help`
and then fails every launch.

```bash
python3 -m pip install debugpy

python3 -c 'import debugpy; print(debugpy.__version__)'
```

Pin a different interpreter — a virtualenv's, say — with `[adapter.debugpy] command` in
`~/.config/lazydap/config.toml`. `lazydap doctor` reports which one it found.

### delve (Go)

```bash
go install github.com/go-delve/delve/cmd/dlv@latest
#    → ~/go/bin/dlv   (make sure that directory is on your PATH)

dlv version
```

lazydap runs it as `dlv dap`, and always with `outputMode: "remote"` — without that every
line the debuggee prints goes to delve's own terminal instead of the DAP stream.

### js-debug (Node.js / Chrome)

```bash
TAG=v1.117.0     # check the latest release
curl -sL -o /tmp/js-debug-dap.tar.gz \
  https://github.com/microsoft/vscode-js-debug/releases/download/$TAG/js-debug-dap-$TAG.tar.gz

mkdir -p ~/.local/opt/js-debug
tar -xzf /tmp/js-debug-dap.tar.gz -C ~/.local/opt/js-debug --strip-components=1
#    DAP entrypoint → ~/.local/opt/js-debug/src/dapDebugServer.js

# The entrypoint is a Node script, not an executable. Wrap it:
mkdir -p ~/.local/bin
cat > ~/.local/bin/js-debug-dap <<'EOF'
#!/usr/bin/env bash
exec node "$HOME/.local/opt/js-debug/src/dapDebugServer.js" "$@"
EOF
chmod +x ~/.local/bin/js-debug-dap

js-debug-dap 0     # port 0 = pick any free port; Ctrl-C to stop
```

### Invocation conventions

No two of them are started the same way:

| Adapter | How lazydap starts it |
|---|---|
| codelldb | `codelldb --port N` |
| debugpy | `python3 -m debugpy.adapter` — stdio, no port at all |
| dlv | `dlv dap --listen=127.0.0.1:N` |
| js-debug-dap | `js-debug-dap N` (positional) |

That variance is intrinsic to the ecosystem. lazydap's per-adapter config carries the right invocation; don't normalise it away in your own scripts either.

### Uninstall

```bash
rm ~/.local/bin/codelldb && rm -rf ~/.local/opt/codelldb
python3 -m pip uninstall debugpy
rm -f ~/go/bin/dlv
rm ~/.local/bin/js-debug-dap && rm -rf ~/.local/opt/js-debug
```

## The non-negotiables

Eight of them, listed with their reasoning in [`AGENTS.md`](AGENTS.md#the-non-negotiables). The two that catch people most often:

- **Every TUI action has a CLI equivalent.** Both wired or neither. This is enforced structurally: `lazydap-tui` cannot depend on `lazydap-daemon`, so a TUI feature that skips the protocol won't compile.
- **JSON output is a product feature.** The schema is a contract other people's tools depend on. Changing one costs an entry in [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md).

## Commits and pull requests

Conventional commits, with an optional milestone scope:

```
feat(m11): the TUI becomes a client of the daemon
fix: five defects found in review of Phase C
docs: record Phase C — TODO ticks, completion notes, D037–D039
```

No attribution footers, no co-author trailers. The body is for why, not what — the diff already says what.

For a pull request:

- **Keep the diff to the change.** Don't refactor adjacent code, delete code that looks unused, or reformat untouched files. Note what you spotted in the PR description instead and let the maintainer decide.
- **All six gates green**, including the boundary script and the skill diff.
- **Architectural change means a decision-log entry.** New `D0NN` in [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md), with the alternatives you rejected and why.
- **A new feature needs a milestone file.** `docs/implementation/tasks/MNN-name.md`, indexed in [`TODO.md`](TODO.md) and the relevant phase doc. Finishing a milestone means ticking its box and adding a completion note to the task file.
- **Say how you verified it**, and against what. "Tests pass" is weaker than "ran the loop against codelldb 1.12.2 on Darwin 25.5.0, output below."

## A note on the book

lazydap is also the subject of a learn-by-LLM Rust book. The chapters under `docs/book/`, the session plan under `docs/teaching/`, the `chapter-*` git tags, and [`.github/workflows/release.yml`](.github/workflows/release.yml) that builds releases from them all belong to `lazydap-learn`, a separate and currently private repository. Edit them there, not here. In this repo they're reference: when you need to know why a piece of code is shaped the way it is, the chapter covering that milestone often explains it better than the blueprint. Product releases use [`.github/workflows/product-release.yml`](.github/workflows/product-release.yml) and `v*` tags, which are unrelated.

## Where to look next

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — crate layout, the IPC contract, the core tenet
- [`AGENTS.md`](AGENTS.md) — non-negotiables, docs structure, how agents work here
- [`docs/implementation/`](docs/implementation/) — the phased build plan; [`TODO.md`](TODO.md) tracks where it's up to
- [`docs/reference/dap-protocol-cheatsheet.md`](docs/reference/dap-protocol-cheatsheet.md) — DAP wire format, quickly
- [`SECURITY.md`](SECURITY.md) — what counts as a vulnerability here, and how to report one
