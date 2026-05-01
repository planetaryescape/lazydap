# M15 — Config file + launch.json import → tag v0.1

## What

1. `~/.config/lazydap/config.toml` — global preferences. Created on first run.
2. `.vscode/launch.json` parsed and surfaced as launch configs.
3. `lazydap launches list` shows configs from both `state.toml` and `launch.json`.
4. **Tag v0.1.0**, publish to crates.io, write README quick-start.

## Why

After M14, the tool works but you have to configure adapters and launches by hand. M15 makes lazydap drop-in usable in any existing repo with `.vscode/launch.json`. Then we ship.

## How

### Step 1 — Config crate

`crates/config/src/lib.rs`:

```rust
pub fn load_config() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let body = std::fs::read_to_string(&path)?;
    Ok(toml::from_str(&body)?)
}

pub fn config_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("LAZYDAP_CONFIG_PATH") {
        return Ok(PathBuf::from(p));
    }
    if let Some(home) = dirs::config_dir() {
        return Ok(home.join("lazydap").join("config.toml"));
    }
    Err("no config dir".into())
}
```

Schema per [`/docs/blueprint/08-state-and-config.md`](../../blueprint/08-state-and-config.md).

### Step 2 — `launch.json` parser

`crates/config/src/launch_json.rs`:

```rust
pub fn parse_launch_json(path: &Path) -> Result<Vec<LaunchConfig>> {
    let body = std::fs::read_to_string(path)?;
    let cleaned = strip_jsonc_comments(&body);     // remove // and /* */ comments
    let parsed: VsCodeLaunchJson = serde_json::from_str(&cleaned)?;
    let mut out = Vec::new();
    for cfg in parsed.configurations {
        out.push(map_to_lazydap_config(cfg)?);
    }
    Ok(out)
}

fn strip_jsonc_comments(s: &str) -> String {
    // Naive: strip // line comments and /* ... */ blocks. Use json5 crate if richer.
}

fn map_to_lazydap_config(c: VsCodeConfig) -> Result<LaunchConfig> {
    let adapter = match c.r#type.as_str() {
        "lldb" | "cppdbg" => AdapterKind::CodeLldb,
        "python" => AdapterKind::DebugPy,
        "node" | "pwa-node" => AdapterKind::JsDebug,
        "go" => AdapterKind::Delve,
        other => AdapterKind::Custom { name: other.into() },
    };
    Ok(LaunchConfig {
        id: LaunchConfigId::new(),
        name: c.name,
        adapter,
        kind: match c.request.as_str() {
            "launch" => LaunchKind::Launch,
            "attach" => LaunchKind::Attach { pid: c.process_id.map(|p| p as i64) },
            _ => return Err("unknown request kind".into()),
        },
        program: c.program.map(|p| substitute_variables(&p)).map(PathBuf::from),
        args: c.args.unwrap_or_default(),
        cwd: c.cwd.map(|p| substitute_variables(&p)).map(PathBuf::from),
        env: c.env.unwrap_or_default(),
        stop_on_entry: c.stop_on_entry.unwrap_or(false),
        source: LaunchConfigSource::VsCodeLaunchJson { name: c.name.clone() },
    })
}

fn substitute_variables(s: &str) -> String {
    // ${workspaceFolder}, ${file}, ${env:VAR} expansion.
    // Be conservative: warn on unresolved variables, don't substitute silently.
}
```

### Step 3 — Surface in CLI

`lazydap launches list --format json` returns combined list (state.toml + launch.json). `lazydap launches run <name>` looks up by name (state.toml takes precedence on conflict, with a warning).

### Step 4 — Release prep

Write/update:

- README.md with v0.1 quick-start, GIF demo
- CHANGELOG.md with v0.1.0 entry
- LICENSE-MIT, LICENSE-APACHE
- CONTRIBUTING.md
- SECURITY.md
- PRIVACY.md
- `Cargo.toml` per crate: `version = "0.1.0"`, `description`, `keywords`, `categories`, `repository`

CI publish workflow that runs on tag push:

```yaml
on:
  push:
    tags: ["v*"]
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo publish -p lazydap-core --token ${{ secrets.CARGO_TOKEN }}
      - run: cargo publish -p lazydap-protocol --token ${{ secrets.CARGO_TOKEN }}
      - run: cargo publish -p lazydap-dap --token ${{ secrets.CARGO_TOKEN }}
      # ... etc, in dependency order
```

### Step 5 — Tag and publish

```bash
git tag v0.1.0
git push origin v0.1.0
# CI publishes to crates.io.
# Or do it manually first time:
cargo publish -p lazydap-core
cargo publish -p lazydap-protocol
cargo publish -p lazydap-dap
cargo publish -p lazydap-config
cargo publish -p lazydap-store
cargo publish -p lazydap-tui
cargo publish -p lazydap-daemon         # binary published last
```

## Success criteria

- `lazydap launches list` shows configs from both sources.
- `.vscode/launch.json` with `${workspaceFolder}` substitutes correctly.
- Unknown variables (`${customVar}`) cause a warning, not silent substitute.
- README quick-start works: a new user can `cargo install lazydap`, drop into a CMake project with `launch.json`, run `lazydap launches list`, pick one, debug.
- v0.1.0 tagged, all crates published to crates.io.
- GitHub release with binary attachments (post-v0.1.0 if release pipeline isn't ready).

## Files

- `crates/config/src/lib.rs`, `launch_json.rs`, `paths.rs` (new)
- `README.md` — overhauled with v0.1 content
- `CHANGELOG.md` — v0.1.0 entry
- `LICENSE-MIT`, `LICENSE-APACHE`
- `CONTRIBUTING.md`, `SECURITY.md`, `PRIVACY.md`
- `.github/workflows/release.yml`
- All `Cargo.toml` files: version 0.1.0, description, etc.

## Verify

```bash
# Fresh machine simulation
cargo install --path crates/daemon
cd ~/code/some-cmake-project        # has .vscode/launch.json
lazydap launches list
lazydap launches run "Debug binary"
# TUI opens, debug session starts.

# Publish dry run:
cargo publish -p lazydap-core --dry-run
```

## Depends on

- [`M14-toggle-breakpoint`](M14-toggle-breakpoint.md).
- All blueprint docs reflect reality.
- README is honest about what v0.1 does and doesn't do.

## Notes

- **Don't ship features post-deadline.** If something's not ready by M15, defer to v0.2. v0.1 doesn't need to be everything.
- **Test `cargo install` on a fresh machine.** Or at least a fresh user. There will be path bugs.
- **Demo GIF matters.** A 30-second GIF showing "open project, set breakpoint, hit it, inspect, fix" sells lazydap better than any prose.
- **After M15, Phase D done. v0.1 in the wild. Phase E begins.**
