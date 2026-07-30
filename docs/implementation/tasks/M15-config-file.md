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

## Release artifacts pre-staged — 2026-07-30 (W5b)

M15's step 4 (release prep) was split off and done ahead of the config code, so the
config/launch.json half lands into a repo whose front door is already right. **M15 is not
complete and its box in `/TODO.md` stays unticked.**

### Landed

- **`README.md`** — rewritten for a v0.1 product. Install from source, the codelldb wrapper-script
  gotcha, a quickstart whose every command and output block was captured from a real run against
  this commit, the TUI, the agent skill, honest scope, and a docs map. Positioned per
  `docs/articles/` — five named trade-offs, each with a defensible opposite, rather than adjectives.
- **`CHANGELOG.md`** — new, Keep a Changelog format. One `[0.1.0] — unreleased` entry describing
  what has landed as user-visible capabilities, plus a Known limitations list.
- **`CONTRIBUTING.md`** — refreshed. Six gates (the four cargo commands plus the boundary script and
  the skill diff), the test layout and why `wait_codelldb.rs` serialises itself, commit and PR
  expectations, adapter install with quirks 1 and 5 called out inline. The chapter-tag machinery is
  now one line pointing at `lazydap-learn`.
- **`SECURITY.md`** — new. Supported versions, private reporting via GitHub advisories, and an
  explicit "a debugger runs arbitrary code by design" scope section separating what is a
  vulnerability from what is the product. Known gaps (chmod-after-bind window, the `openat` TOCTOU,
  no peer-credential check, umask-default `.lazydap/`) stated rather than hidden.
- **`PRIVACY.md`** — new, short. No telemetry, no network, what is written to disk and where.
- **`.github/workflows/product-release.yml`** — new, dormant until a `v*` tag exists. Gates →
  build matrix (macOS arm64 + x86_64, Linux x86_64) → GitHub Release with tarballs, SHA-256 sums and
  `lazydap.skill` attached, notes generated from the CHANGELOG section for the tag. Verifies the tag
  matches the workspace version before building anything. `actionlint` clean.
  **`release.yml` was not touched** — it is the teaching-era `chapter-*` workflow and `lazydap-learn`
  owns its semantics.
- **Workspace metadata** — `[workspace.package]` gains `description`, `keywords` and `categories`;
  every crate inherits those and sets its own `description`. `publish = false` stays on all seven.

### Found while verifying, not fixed (no code changes in this pass)

- **Conditional breakpoints already work from the CLI.** `break --condition 'i == 7'` binds and
  stops correctly against codelldb, though `TODO.md` and the blueprint still list them as
  post-v0.1. The TUI cannot set one. Docs now say so; the roadmap wording is stale.
- **A debuggee under `/tmp` on macOS never binds a breakpoint.** `/tmp` is a symlink to
  `/private/tmp`; codelldb reports `verified: false` with "could not be resolved, but a valid
  location was found at /tmp/...", and the program runs straight through. Candidate quirk 8, and a
  candidate for canonicalising source paths in the store.
- **`variables_reference` values go stale on every stop**, and a stale one returns
  `DapProtocolError: Invalid variabes reference` (codelldb's typo). Correct DAP behaviour, but
  `skill/SKILL.md` does not warn about it and an agent will lose a turn to it.

### Still to do for M15

1. The config-code half: `crates/config` global `config.toml`, `.vscode/launch.json` parsing with
   `${workspaceFolder}` substitution, `lazydap launches list` / `run`.
2. `docs/blueprint/15-decision-log.md`: resolve the open crates.io question. The workflow has no
   publish job and says why in a trailing comment.
3. The demo GIF.
4. **Finalise the `[0.1.0]` CHANGELOG wording as part of cutting the tag** (Wave 6). The section
   currently says "unreleased" and "until the tag is cut", and `product-release.yml` copies the
   section verbatim onto the release page. It refuses to publish while that wording is still
   there, so this is a blocking step rather than a nicety: drop the unreleased preamble and date
   the heading (`## [0.1.0] — 2026-MM-DD`) in the same commit that precedes the tag.
5. Tag `v0.1.0` — only after M12–M14 land, since the README's roadmap says those panes are next.
