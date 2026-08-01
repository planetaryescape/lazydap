---
title: Install lazydap
description: Install codelldb and lazydap by Homebrew, script or source, then check both with doctor.
---

Install codelldb first, then lazydap, then run `lazydap doctor` to confirm each piece is
where lazydap expects it.

## Prerequisites

macOS or Linux. Windows is not a target. Released builds cover macOS arm64 and x86_64 and
Linux x86_64; anything else builds from source.

Building from source needs a **Rust toolchain**, from [rustup](https://rustup.rs/). The
pinned channel comes from `rust-toolchain.toml` on the first `cargo` command you run in the
repository, so there is nothing to choose. Homebrew and the install script do not need it.

## 1. Install codelldb

codelldb is the debug adapter that does the actual debugging. lazydap drives it and does not
bundle it.

Releases ship as `.vsix` files, which are renamed zips. VS Code is not involved.

```bash
curl -sL -o /tmp/codelldb.vsix \
  https://github.com/vadimcn/codelldb/releases/latest/download/codelldb-darwin-arm64.vsix
mkdir -p ~/.local/opt/codelldb
unzip -q -o /tmp/codelldb.vsix -d ~/.local/opt/codelldb
```

Other platforms: `codelldb-linux-x64.vsix`, `codelldb-linux-arm64.vsix`,
`codelldb-darwin-x64.vsix`.

Now put it on `PATH` **with a wrapper script, not a symlink**:

```bash
mkdir -p ~/.local/bin
cat > ~/.local/bin/codelldb <<'EOF'
#!/usr/bin/env bash
exec "$HOME/.local/opt/codelldb/extension/adapter/codelldb" "$@"
EOF
chmod +x ~/.local/bin/codelldb
```

:::danger[`ln -s` will not work]
codelldb finds `liblldb` by walking up from `argv[0]`. Through a symlink that walk starts one
directory too high, and it dies in `dlopen` before it does anything. The wrapper hands it an
absolute path. Full write-up: [quirk 1](/reference/codelldb-quirks/#1-a-symlink-on-path-breaks-liblldb-resolution).
:::

Check it runs. `--version` is not a flag codelldb knows, so use `--help`:

```bash
codelldb --help
```

If that hangs with no output at all, see
[quirk 5](/reference/codelldb-quirks/#5-every-invocation-hangs-after-a-macos-update) — a macOS
update can wedge the binary, and the fix is a re-copy.

## 2. Install lazydap

Whichever route you take, it installs one binary, `lazydap`. The daemon, the CLI and the TUI
are all inside it.

### Homebrew

```bash
brew install planetaryescape/lazydap/lazydap
```

The formula comes from the `planetaryescape/homebrew-lazydap` tap, which the release
workflow updates as part of cutting a version.

### Install script

```bash
curl -fsSL https://raw.githubusercontent.com/planetaryescape/lazydap/main/install.sh | bash
```

It reads `uname`, downloads the matching release build, and checks its SHA-256 **before**
unpacking it — a checksum checked afterwards only tells you what you already extracted. The
binary lands in `~/.local/bin`; set `LAZYDAP_INSTALL_DIR` to put it somewhere else. Nothing
in it uses `sudo`.

Pass a tag to pin a version rather than take the newest:

```bash
curl -fsSL https://raw.githubusercontent.com/planetaryescape/lazydap/main/install.sh | bash -s -- v0.1.0
```

### From source

```bash
git clone https://github.com/planetaryescape/lazydap
cd lazydap
cargo install --path crates/daemon
```

## 3. Check the pieces

```console
$ lazydap doctor --format json
{
  "checks": [
    {
      "detail": "none; create /Users/you/.config/lazydap/config.toml to add one",
      "name": "config.file",
      "ok": true
    },
    {
      "detail": "/Users/you/.local/bin/codelldb",
      "name": "adapter.codelldb",
      "ok": true
    },
    {
      "detail": "/opt/homebrew/bin/python3",
      "name": "adapter.debugpy",
      "ok": true
    },
    {
      "detail": "/Users/you/go/bin/dlv",
      "name": "adapter.delve",
      "ok": true
    },
    {
      "detail": "/Users/you/lazydap-demo/.lazydap/state.toml (not created yet)",
      "name": "state.file",
      "ok": true
    },
    {
      "detail": "instance lazydap-demo-13cc8efcde46, pid 53846, protocol v7",
      "name": "daemon",
      "ok": true
    }
  ],
  "ok": true
}
```

Six checks: whether a config file was found, one per adapter, whether the project's breakpoint
file is readable, and whether a daemon is answering. `"ok": true` on all six means you can
debug anything lazydap supports.

You do not need all three adapters. An `adapter.*` check that is `false` only rules out that
language — `"ok": false` overall, exit code 1, and everything else still works. Install the
one you need and run `doctor` again.

`doctor` started that daemon. So does every other command — there is nothing to launch by
hand, and [the daemon guide](/guides/daemon/) explains what it does while it is there.

If an adapter check is `false`, lazydap looked in the config file first and then on `PATH`, and
found nothing either way. `config.file` shows which file it read, or where to create one.

## 4. Shell completions

```bash
lazydap completions zsh > ~/.zfunc/_lazydap    # zsh
lazydap completions bash > ~/.local/share/bash-completion/completions/lazydap
```

`fish`, `elvish` and `powershell` also work.

## What you get

Three adapters, each of which you install yourself — lazydap drives them, it does not
bundle them. `lazydap doctor --check-adapters` reports which ones this machine has.

| Language | Adapter | How to get it |
|---|---|---|
| C, C++, Rust | codelldb | the [CodeLLDB releases page](https://github.com/vadimcn/codelldb/releases), or your package manager |
| Python | debugpy | `python3 -m pip install debugpy` — lazydap looks for an *interpreter* that can import it |
| Go | delve | `go install github.com/go-delve/delve/cmd/dlv@latest` |

js-debug, for Node, is not built yet.

:::caution[delve needs `GOPATH/bin` on your `PATH`]
`go install` puts `dlv` in `$(go env GOPATH)/bin`, which is not on `PATH` by default on
most machines. If `doctor` says delve is missing on a machine you just installed it on,
that is almost always why — add the directory and try again.

`mode: "debug"` (what a `.go` file gets) also shells out to `go build`, so it needs a Go
toolchain, not just `dlv`. An already-compiled binary runs under `mode: "exec"` and needs
neither.
:::

## Next

- [Quickstart](/getting-started/quickstart/) — a breakpoint and a variable, in about a minute
- [Troubleshooting](/troubleshooting/) — if `doctor` came back unhappy
