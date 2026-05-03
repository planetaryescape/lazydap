# Contributing to lazydap

How to set up a working dev environment for this repo. For project conventions (commit style, testing rules, teaching mode) see [`AGENTS.md`](AGENTS.md). For architecture see [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Prerequisites

- **Rust toolchain** — pinned in [`rust-toolchain.toml`](rust-toolchain.toml) (stable channel, `rustfmt` + `clippy`). Install via [rustup](https://rustup.rs/); the toolchain file makes `cargo` auto-select the right version on first invocation in this directory.
- **macOS or Linux** — Windows is not currently a target.
- **A DAP adapter or two** — see below. Not strictly required for the earliest milestones (M0–M2 lean on an in-process `FakeAdapter`), but you'll need real adapters from M3 onwards and to run the canonical integration tests.

## Build, test, lint

```bash
cargo build --workspace
cargo test --workspace                          # or: cargo nextest run --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

CI runs all four. Run them locally before pushing.

## DAP adapters

lazydap is a wrapper around external DAP adapter processes. To exercise the real code paths (and run the canonical integration tests once they exist), you need at least one adapter installed and on `PATH`. The three primary targets:

| Adapter | Languages | Source |
|---|---|---|
| **codelldb** | Rust, C, C++, Swift, anything LLDB can debug | [vadimcn/codelldb](https://github.com/vadimcn/codelldb) |
| **debugpy** | Python | [microsoft/debugpy](https://github.com/microsoft/debugpy) (PyPI) |
| **js-debug** | Node.js, Chrome, Edge | [microsoft/vscode-js-debug](https://github.com/microsoft/vscode-js-debug) |

All three speak DAP over TCP. lazydap will spawn them as child processes and connect to a port they print on startup.

The convention used below: third-party prebuilt blobs go in `~/.local/opt/<name>/`, executables get exposed on `PATH` via `~/.local/bin/`. Make sure `~/.local/bin` is on your `PATH` (`echo $PATH | tr ':' '\n' | grep .local/bin`). Adjust paths if you prefer a different layout — nothing in lazydap depends on these specific locations.

### codelldb (native: Rust / C / C++)

Upstream releases ship platform-specific `.vsix` bundles that contain the prebuilt `codelldb` binary plus its `liblldb.dylib` / `liblldb.so`. A `.vsix` is just a renamed zip — VS Code is **not** required to use the contents.

Pick the asset matching your platform from the [latest release](https://github.com/vadimcn/codelldb/releases/latest). The Apple Silicon flow:

```bash
# 1. Download the platform-specific bundle
curl -sL -o /tmp/codelldb.vsix \
  https://github.com/vadimcn/codelldb/releases/latest/download/codelldb-darwin-arm64.vsix

# 2. Extract
mkdir -p ~/.local/opt/codelldb
unzip -q -o /tmp/codelldb.vsix -d ~/.local/opt/codelldb
#    binary  → ~/.local/opt/codelldb/extension/adapter/codelldb
#    liblldb → ~/.local/opt/codelldb/extension/lldb/lib/liblldb.dylib

# 3. Expose on PATH via a wrapper script (NOT a symlink — see note below).
cat > ~/.local/bin/codelldb <<'WRAPPER_EOF'
#!/usr/bin/env bash
exec "$HOME/.local/opt/codelldb/extension/adapter/codelldb" "$@"
WRAPPER_EOF
chmod +x ~/.local/bin/codelldb

# 4. Verify
codelldb --help            # NOTE: --version is not a recognised flag
```

> **⚠️ Don't use `ln -sf` here.** codelldb computes the `liblldb.dylib` path at runtime relative to `argv[0]`. When invoked through a symlink at `~/.local/bin/codelldb`, the relative path computation goes wrong and codelldb panics with a `dlopen` failure. The wrapper-script approach above invokes the real binary with an absolute path, so the relative computation lands correctly. Full forensic write-up: [`docs/reference/codelldb-quirks.md`](docs/reference/codelldb-quirks.md) (quirk #1).

Asset names for other platforms: `codelldb-linux-x64.vsix`, `codelldb-linux-arm64.vsix`, `codelldb-darwin-x64.vsix`.

> **Don't use `code --install-extension vadimcn.vscode-lldb` and expect a CLI binary.** That marketplace package is a thin shim that lazy-downloads the platform binary on first activation *inside VS Code*. Useful for in-IDE debugging, useless for standalone CLI use.

### debugpy (Python)

Microsoft's official Python debug adapter. Same package VS Code's Python extension uses under the hood. Easiest install is via [`pipx`](https://pipx.pypa.io/) (isolated venv per tool, console scripts auto-symlinked to `~/.local/bin`):

```bash
pipx install debugpy
#    → venv at ~/.local/pipx/venvs/debugpy/
#    → ~/.local/bin/debugpy           (for `python -m debugpy`-style use)
#    → ~/.local/bin/debugpy-adapter   (this is the DAP-over-TCP entrypoint)

debugpy-adapter --help
```

If you don't want pipx: `pip install --user debugpy` works too, but you'll need to make sure the user-site `bin/` is on `PATH`.

### js-debug (Node.js / Chrome)

Microsoft's official JS/TS debugger, same one bundled into VS Code. They publish a standalone-friendly tarball on each release exactly for out-of-IDE use.

```bash
# 1. Download (replace v1.117.0 with current latest)
TAG=v1.117.0
curl -sL -o /tmp/js-debug-dap.tar.gz \
  https://github.com/microsoft/vscode-js-debug/releases/download/$TAG/js-debug-dap-$TAG.tar.gz

# 2. Extract (the --strip-components=1 drops the top-level js-debug/ dir)
mkdir -p ~/.local/opt/js-debug
tar -xzf /tmp/js-debug-dap.tar.gz -C ~/.local/opt/js-debug --strip-components=1
#    DAP entrypoint → ~/.local/opt/js-debug/src/dapDebugServer.js

# 3. The entrypoint is a Node script, not an executable. Wrap it:
cat > ~/.local/bin/js-debug-dap <<'EOF'
#!/usr/bin/env bash
exec node "$HOME/.local/opt/js-debug/src/dapDebugServer.js" "$@"
EOF
chmod +x ~/.local/bin/js-debug-dap

# 4. Smoke-test (port 0 = pick any free port; prints "Debug server listening at ::1:<port>")
js-debug-dap 0
# Ctrl-C to stop.
```

Requires a working `node` on `PATH` (any recent LTS).

### Filesystem layout summary

```
~/.local/bin/
├── codelldb            → ~/.local/opt/codelldb/extension/adapter/codelldb   (symlink)
├── debugpy             → ~/.local/pipx/venvs/debugpy/...                    (pipx)
├── debugpy-adapter     → ~/.local/pipx/venvs/debugpy/...                    (pipx)
└── js-debug-dap                                                             (wrapper script)

~/.local/opt/
├── codelldb/           # full bundle: codelldb binary + liblldb
└── js-debug/           # Node DAP server

~/.local/pipx/venvs/debugpy/   # debugpy's isolated Python env
```

### Invocation conventions (read this once, save yourself confusion later)

The three adapters take **different** invocation conventions for the same conceptual operation ("listen on a port for DAP traffic"):

| Adapter | "Listen on port N" |
|---|---|
| codelldb | `codelldb --port N` |
| debugpy-adapter | `debugpy-adapter --port N` |
| js-debug-dap | `js-debug-dap N` (positional) |

This variance is intrinsic to the adapter ecosystem, not a wart lazydap should iron out. lazydap's adapter spec config carries the right invocation per adapter; don't normalise it away in your own scripts either — when you script against an adapter directly, use whatever convention that adapter actually accepts.

### Uninstall

```bash
# codelldb
rm ~/.local/bin/codelldb
rm -rf ~/.local/opt/codelldb

# debugpy
pipx uninstall debugpy

# js-debug
rm ~/.local/bin/js-debug-dap
rm -rf ~/.local/opt/js-debug
```

## Chapter tags and releases

This repo is also a **learn-by-LLM book** (see [`docs/book/`](docs/book/)). Every chapter has a git tag named `chapter-NN` that points at the state of `main` you should checkout to **begin** that chapter. Each tag has a corresponding GitHub Release.

### How the convention works

- **Naming**: `chapter-NN` is the *start state* of chapter NN. To start chapter 04, run `git checkout chapter-04`. The end of chapter NN is the start of chapter NN+1, so to see the state *after* a chapter ships, checkout the next chapter's tag (or `main` for the latest taught chapter).
- **What it points at**: the commit on `main` that represents "everything taught up to and including chapter NN-1, ready to start chapter NN."
- **When to create**: at the end of a session that completes chapter NN-1, tag the resulting commit as `chapter-NN`. Don't tag the commit being authored — tag the state it produces, which is the starting point for the *next* chapter.
- **Releases auto-create**: pushing a `chapter-*` tag fires [`.github/workflows/release.yml`](.github/workflows/release.yml), which generates release notes from the chapter file's frontmatter and creates the GitHub Release.

### Best-current semantics (tags move forward)

When a retroactive fix lands on `main` that affects an earlier chapter (e.g., a milestone file gets rewritten because of version drift in a tool), **move the affected chapter tags forward**:

```bash
git tag -fa chapter-04 <new-commit>
git push --force-with-lease origin chapter-04
```

The release workflow handles updates idempotently — if the release already exists, its notes are refreshed; if not, it's created.

This means `chapter-NN` tags always point at the *best-current* version of that chapter's start state, not the historical "as-shipped" version. The chapter narrative still tells the story of any pedagogically valuable bugs we hit live; the code at the tagged checkout point reflects the corrected state so future learners don't re-encounter accidents that don't teach anything.

### Workspace-setup wart

Chapters 01-03 (cargo workspaces, async main + clap, conventions as code) ended up folded into a single workspace-setup commit during initial scaffolding, so they don't have individual `chapter-01`, `chapter-02`, `chapter-03` tags — there's no separate commit for each. The first separately-tagged checkout point is `chapter-04`. To start at chapter 01, work from a fresh `git init` instead. Going forward, each session ships in its own commit so this won't recur.

### Why this exists

A book is built linearly across many sessions, but learners don't always want to read it linearly. Chapter tags let someone arrive cold at chapter 09, checkout the corresponding tag, and have the codebase in exactly the right shape to follow the chapter. Same affordance for the LLM-as-teacher mode: an agent picking up a session can verify it's at the right starting state by `git checkout chapter-NN`.

## Where to look next

- [`AGENTS.md`](AGENTS.md) — non-negotiables, commit style, teaching mode, docs structure
- [`ARCHITECTURE.md`](ARCHITECTURE.md) — crate layout, IPC contract, the core tenet
- [`docs/implementation/`](docs/implementation/) — phased build plan; [`/TODO.md`](TODO.md) tracks current state
- [`docs/reference/dap-protocol-cheatsheet.md`](docs/reference/dap-protocol-cheatsheet.md) — DAP wire format quick reference
