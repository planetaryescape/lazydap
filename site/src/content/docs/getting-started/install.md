---
title: Install lazydap
description: Install codelldb and build lazydap from source, then check both with doctor.
---

Install codelldb first, then lazydap, then run `lazydap doctor` to confirm each piece is
where lazydap expects it. There are no published binaries yet, so lazydap is a `cargo
install` from a clone.

## Prerequisites

A **Rust toolchain**, from [rustup](https://rustup.rs/). The pinned channel comes from
`rust-toolchain.toml` on the first `cargo` command you run in the repository, so there is
nothing to choose.

macOS or Linux. Windows is not a target.

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

```bash
git clone https://github.com/planetaryescape/lazydap
cd lazydap
cargo install --path crates/daemon
```

That installs one binary, `lazydap`. The daemon, the CLI and the TUI are all inside it.

## 3. Check the pieces

```console
$ lazydap doctor --format json
{
  "checks": [
    {
      "detail": "/Users/you/.local/bin/codelldb",
      "name": "adapter.codelldb",
      "ok": true
    },
    {
      "detail": "/Users/you/lazydap-demo/.lazydap/state.toml (not created yet)",
      "name": "state.file",
      "ok": true
    },
    {
      "detail": "instance lazydap-demo-13cc8efcde46, pid 2452, protocol v2",
      "name": "daemon",
      "ok": true
    }
  ],
  "ok": true
}
```

Three checks: the adapter is on `PATH`, the project's breakpoint file is readable, and a
daemon is answering. `"ok": true` on all three means you can debug something.

`doctor` started that daemon. So does every other command — there is nothing to launch by
hand, and [the daemon guide](/guides/daemon/) explains what it does while it is there.

If `adapter.codelldb` is `false`, lazydap searched `PATH` and found nothing. Adapter discovery
is `PATH`-only today; the config file that would let you point at an arbitrary path is
specified and not built.

## 4. Shell completions

```bash
lazydap completions zsh > ~/.zfunc/_lazydap    # zsh
lazydap completions bash > ~/.local/share/bash-completion/completions/lazydap
```

`fish`, `elvish` and `powershell` also work.

## What you get

C, C++ and Rust, through codelldb. debugpy for Python, delve for Go and js-debug for
Node are planned and not built, so a Python program has nothing to run it under today.

## Next

- [Quickstart](/getting-started/quickstart/) — a breakpoint and a variable, in about a minute
- [Troubleshooting](/troubleshooting/) — if `doctor` came back unhappy
