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

## Completion note — 2026-07-30 (W6, the code half)

**Done.** `crates/config` grew a config loader and a `launch.json` importer, `crates/core` grew the
`LaunchConfig` vocabulary, `crates/store` reads `[[launch_configs]]`, and the CLI grew
`lazydap launches list` / `run`. Quirk 8 — the `/tmp` breakpoint failure the milestone inherited —
is fixed rather than documented. Four gates green, boundary script green, 474 tests (415 baseline
plus 59), verified live against real codelldb.

### Deviations from the plan above

- **`AdapterKind` gained no variants.** The sketch mapped `python`/`node`/`go` onto
  `AdapterKind::DebugPy`/`JsDebug`/`Delve` and unknown types onto `Custom { name }`. Those variants
  do not exist and inventing them would have been four dead arms and a `Custom` nothing consumes.
  A configuration for another debugger keeps its `type` string in `LaunchConfig::adapter_type` with
  `adapter: None`, and is **listed** with the reason it cannot run. `cppdbg` maps to codelldb with a
  warning saying its `MIMode` and `setupCommands` are ignored.
- **No `LaunchConfigId`.** Lookup is by name, which is what the CLI takes and what both files
  carry. An id nothing indexes by is a field to keep unique for no reason.
- **The config schema is smaller than `08-state-and-config.md`.** Implemented: `[adapter.<name>]
  command` and `[general] wait_timeout_seconds`, both because something consumes them. Not
  implemented: themes, keymaps, log rotation, socket directories, output defaults, and
  `default_adapter` — which with one adapter kind can only ever hold `"codelldb"`. Unknown keys are
  ignored rather than rejected, and there is a test that the blueprint's own example parses.
- **D026's middle tier is still absent.** Discovery is config, then `PATH`. The managed
  `{data_dir}/adapters/` directory is skipped for the reason M5 skipped it: nothing installs an
  adapter into it, so a lookup there is dead code dressed as policy. A pinned path that is not
  executable is now an *error* rather than a fall-through to `PATH`.
- **`launches` is client-side and adds no protocol request** (D047), so the protocol stays at v3.
- **`[[launch_configs]]` are read-only**, out of the store's `unknown` table. Modelling them as a
  typed field would have deleted hand-written ones on the next breakpoint write — the round-trip
  that already protects them is what makes the read safe.
- **The skill generator now recurses.** `launches` is the first nested subcommand, and
  `gen_skill_commands` documented only the top level — so `launches list` and `launches run` were
  absent from the file whose whole purpose is that it cannot omit what the parser accepts. It now
  walks children and skips clap's own `help`, which also removed a meaningless `### lazydap help`
  section.
- **`TODO.md` had unresolved merge-conflict markers** in its Phase D block, committed by the Phase D
  merge. Resolved here in favour of the completed entries, which is what the rest of the file and
  `main`'s history already say.

### New decisions

- **D046** — the JSONC dialect is read by a hand-rolled, string-aware scanner rather than a
  dependency, and only VS Code's dialect (comments, trailing commas), not JSON5's.
- **D047** — launch configurations are resolved by the client, and `run` sends an ordinary `Launch`.
- **D048** — an unbound breakpoint is re-sent under the path the adapter names, once, only when
  nothing in that file bound and only when the suggestion resolves to the same file.

### Review round — 2026-07-30, six findings, all fixed

1. **The adapter pin was resolved in the wrong process.** Discovery ran inside the daemon, under the daemon's environment, so `LAZYDAP_CONFIG_PATH=... lazydap launch` read the pin client-side and then the long-lived daemon resolved the adapter again against its own default path and fell through to `PATH`. The client now resolves it and sends it in `LaunchRequest` (**D050**), and the **protocol goes to v4** — an optional field would be *ignored* by a stale same-version daemon, which is the bug wearing a compatibility hat.
2. **cppdbg configurations lost their environment.** cppdbg spells it `environment: [{name, value}]` and its entry stop `stopAtEntry`; both were ignored while the configuration was still declared runnable, so a program needing `LD_LIBRARY_PATH` launched without it and nothing said so. Both spellings are now mapped, and the warning names what is still ignored (`MIMode`, `miDebuggerPath`, `setupCommands`).
3. **The documented config path was wrong on macOS.** `dirs::config_dir()` is `~/Library/Application Support`; the docs said `~/.config`. Now searched in order — `LAZYDAP_CONFIG_PATH`, `$XDG_CONFIG_HOME`, `~/.config`, then the platform directory — first that *exists* winning (**D049**). README, CHANGELOG and blueprint 08 all say the same thing.
4. **`args` as a shell string was dropped.** codelldb accepts one; such configurations were discarded as unreadable. A small quote-aware splitter handles it, and an unterminated quote makes the configuration *unrunnable with that reason* rather than silently mis-split.
5. **A config typo bricked the recovery commands.** Every command parsed the config before dispatch, so a misplaced bracket took `status`, `shutdown`, `disconnect` and `logs` down with it — the commands you reach for while a debuggee is running. Only launch-class commands now require it (`Command::needs_config`); the rest warn and carry on with the defaults, and `doctor` reports it as a failed `config.file` check with the path and the parse error.
6. **The JSONC stripper deleted comments instead of blanking them.** `tr/*x*/ue` became `true` — a document VS Code rejects, quietly accepted. Comments now become the whitespace they occupied, character for character, which also keeps parse errors pointing at the right column. An unterminated comment or string is refused rather than accepted.

### Still to do at tag time (not code)

1. Answer the crates.io question and record it as a D-entry. `publish = false` on all seven crates
   is the standing default and the workflow has no publish job.
2. Re-date `## [0.1.0] — 2026-07-30` in `CHANGELOG.md` if the tag is cut on another day.
3. `git tag v0.1.0 && git push origin v0.1.0`. Everything downstream of that is automated.
4. The demo GIF (step 4 above) is still unmade. Not a blocker.
