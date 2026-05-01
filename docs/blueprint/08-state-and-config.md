# 08 — State and config

Two TOML files:

1. **`.lazydap/state.toml`** — per-project state. Breakpoints, watches, named launch configs. Should be committed (or ignored) per team preference.
2. **`~/.config/lazydap/config.toml`** — per-user preferences. Default adapter paths, theme, log level. Never committed.

Plus one read-only:

3. **`.vscode/launch.json`** — imported as additional launch configs.

## `.lazydap/state.toml` (per project)

Lives at the project root. Auto-detected (see "Project root detection" below). Created on first `lazydap config init`.

```toml
# Schema version
version = 1

# Per-project breakpoints
[[breakpoints]]
id = "bp-01ABCDEFGHIJKLMNOPQRSTUVWXYZ"
source = "src/main.c"        # path relative to project root
line = 42
condition = "x > 5"          # optional
hit_condition = ">= 10"      # optional
log_message = "x = {x}"      # optional, mutually exclusive with normal pause
enabled = true

[[breakpoints]]
id = "bp-01XYZABCDEFGHIJKLMNOPQRSTU"
source = "src/foo.c"
line = 101
enabled = true

# Per-project watches
[[watches]]
id = "watch-01..."
expression = "tokens[pos]"
label = "current token"      # optional
enabled = true

# Per-project named launch configs
[[launch_configs]]
id = "lc-01..."
name = "main"
adapter = "codelldb"
kind = "launch"
program = "build/Debug/c_beans"
args = ["--verbose"]
cwd = "."                    # relative to project root
stop_on_entry = false

[launch_configs.env]
RUST_LOG = "debug"

# Per-project adapter overrides (optional)
[adapter.codelldb]
command = "/opt/codelldb/codelldb"
extra_args = ["--port", "0"]

# Per-project preferences (optional)
[preferences]
default_launch_config = "main"
```

### Why TOML, not SQLite

(Per [`15-decision-log.md`](15-decision-log.md) D006.)

- **Human-readable.** A team member can open `.lazydap/state.toml`, see "we have 3 breakpoints in main.c", reason about it.
- **Version-controllable.** Commit it if you want shared breakpoints. Don't if you don't.
- **Scriptable from any language.** Python: `tomllib`. JS: `@iarna/toml`. Bash: `dasel`. No `.db` driver needed.
- **No migrations.** Add a field, default it on read.
- **Diffable.** PR reviews show breakpoint changes as text diffs.

The trade-off: if you have 10,000 breakpoints in one project, TOML reads slow. Cross that bridge when we get there.

### Read/write semantics

- **Reads:** on daemon startup, the state file is fully parsed into RAM. Stays cached.
- **Writes:** debounced. Every mutation marks the in-memory state dirty; a background task flushes to disk every 500ms (or on graceful shutdown). Avoids hammering the disk if a user sets 20 breakpoints quickly.
- **Atomicity:** writes go to `.lazydap/state.toml.tmp`, then rename. No half-written files.
- **Conflict resolution:** if the file changed on disk between reads (user edited externally), the daemon detects via mtime and merges in (mostly: external edits to breakpoints take precedence; ties go to file).

### What goes in `.lazydap/state.toml` vs `.vscode/launch.json`

- `.lazydap/state.toml`: **lazydap's own state** — breakpoints, watches, lazydap-native launch configs.
- `.vscode/launch.json`: **inherited from VS Code**. lazydap reads it, doesn't write it. Treats configurations as additional named launch configs (with `source: VsCodeLaunchJson { name }`).

Both can coexist. `lazydap launches list` shows configs from both sources.

## `~/.config/lazydap/config.toml` (per user)

Global preferences. Override-able by `LAZYDAP_*` env vars. Never per-project.

XDG-compliant paths:

- macOS: `~/Library/Application Support/lazydap/config.toml`
- Linux: `$XDG_CONFIG_HOME/lazydap/config.toml` or `~/.config/lazydap/config.toml`
- Windows: `%APPDATA%\lazydap\config.toml` (post-v0.1; v0.1 may be Unix-only)

```toml
version = 1

[general]
default_adapter = "codelldb"
log_level = "info"            # trace, debug, info, warn, error
log_to_file = true
log_max_size_mb = 100
log_max_files = 5

[daemon]
idle_shutdown_minutes = 0     # 0 = disabled
socket_dir = ""               # default: $XDG_RUNTIME_DIR or ~/Library/Application Support/lazydap

[output]
default_format = "auto"       # auto = TTY-detect, or table|json|...
table_use_colors = "auto"     # auto = isatty, or always|never
json_pretty = false           # false = single-line for piping

[adapter.codelldb]
command = "/usr/local/bin/codelldb"
extra_args = []

[adapter.debugpy]
command = "debugpy-adapter"   # PATH lookup
extra_args = []

[tui]
keymap = "vim"                # vim | emacs (post-v0.1)
theme = "default"
tick_rate_ms = 16
syntax_highlight = true
```

## Env var overrides

| Env var | Overrides |
|---|---|
| `LAZYDAP_INSTANCE` | Project root detection — use as instance key |
| `LAZYDAP_SOCKET_PATH` | Daemon socket path |
| `LAZYDAP_DATA_DIR` | Daemon data dir (logs, PID file) |
| `LAZYDAP_CONFIG_PATH` | Path to `~/.config/lazydap/config.toml` |
| `LAZYDAP_LOG_LEVEL` | Override log level for this invocation |
| `LAZYDAP_LOG_DAP` | If set, log all DAP traffic (verbose) |
| `LAZYDAP_TIMEOUT` | Default `--wait` timeout in seconds |
| `LAZYDAP_NO_DAEMON_AUTO_SPAWN` | If set, don't auto-spawn daemon |
| `RUST_LOG` | Standard `tracing` filter |

Inherited mostly from mxr's `MXR_*` conventions.

## Project root detection

Walk up from cwd. First match wins:

1. **`.lazydap/`** directory exists — use this as project root.
2. **`.git/`** directory exists — use this.
3. **Language manifests** (`Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`, `CMakeLists.txt`) — use the directory containing them.
4. **`cwd`** itself — fallback.

Open question (per [`15-decision-log.md`](15-decision-log.md) O01): in nested git repos (worktrees, submodules), which level wins? Default: closest. Configurable via `LAZYDAP_INSTANCE=/path/to/root` if user wants something different.

The detected project root becomes the instance key. Multiple invocations from inside the same project hit the same daemon.

## `.vscode/launch.json` import

Read on session startup. Parsed with comment-tolerant JSON dialect (`json5`-like; we use `json5` crate or `serde_json` after stripping comments). Mapped to lazydap's `LaunchConfig` shape.

VS Code launch.json shape (subset):

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "Debug binary",
      "program": "${workspaceFolder}/build/Debug/c_beans",
      "args": [],
      "cwd": "${workspaceFolder}",
      "stopOnEntry": false,
      "env": { "RUST_LOG": "debug" }
    }
  ]
}
```

Mapping rules:

- `type: "lldb"` → `AdapterKind::CodeLldb`
- `type: "python"` → `AdapterKind::DebugPy`
- `type: "node"` / `"pwa-node"` → `AdapterKind::JsDebug`
- `type: "go"` → `AdapterKind::Delve`
- Unknown `type` → `AdapterKind::Custom { name }` with a warning
- `${workspaceFolder}` → project root
- `${file}` → currently-open file in TUI (only for TUI launches; CLI rejects unresolved variables)
- `${env:VAR}` → expand from process env

`lazydap launches list` shows imported configs with `source: VsCodeLaunchJson`.

## State migrations

Schema version bumps require migration logic. v0.1 ships with `version = 1`. Future bumps:

- v1 → v2: write a `crates/store/src/migrate.rs::migrate_v1_to_v2(state)` function. Run on read if version mismatch.
- Don't migrate destructively. Save a backup at `.lazydap/state.toml.v1.bak` before rewriting.

## Defaults and "first-run" UX

- No `~/.config/lazydap/config.toml` → use compiled-in defaults, write nothing.
- No `.lazydap/state.toml` in project → run with empty state. `lazydap config init` creates a starter file.
- No `.vscode/launch.json` → fine. lazydap-native launch configs only.
- Adapter not found → `lazydap doctor` reports clearly which paths were tried.

## Privacy notes

- No telemetry. No phone-home. The daemon doesn't know about lazydap upstream.
- Logs may contain code paths and source snippets; treat the log file as sensitive.
- `.lazydap/state.toml` may contain expressions ("eval `internal_secret_key()`"); treat as sensitive if your team commits it.

## See also

- [`02-data-model.md`](02-data-model.md) — types serialised here
- [`04-protocol.md`](04-protocol.md) — `LaunchConfigList`, `BreakpointList`, etc.
- [`15-decision-log.md`](15-decision-log.md) — D006, D008
