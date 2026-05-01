# M7 — Skill + agent verification

## What

1. Build `lazydap.skill` ZIP at repo root containing `SKILL.md` + `references/commands.md` + `references/examples.md`.
2. Auto-generate `references/commands.md` from clap help.
3. Verify end-to-end with Claude Code: agent reads skill, drives a debug session, reports findings.

## Why

This is the moment lazydap becomes useful for AI agents. After M7, you can hand the skill to any agent that reads `.skill` files and they can debug code.

## How

### Step 1 — Skill generator script

`crates/daemon/scripts/generate-skill.rs` (build-script style — runs in CI before packaging):

```rust
fn main() {
    // Walk clap subcommands, emit references/commands.md
    let cmd = lazydap_daemon::cli::root_command();
    let mut out = String::new();
    out.push_str("# lazydap commands\n\n");
    for sub in cmd.get_subcommands() {
        out.push_str(&format!("## `{}`\n\n", sub.get_name()));
        // Render each subcommand: usage, args, examples
        ...
    }
    std::fs::write("lazydap.skill/references/commands.md", out).unwrap();
}
```

### Step 2 — Hand-write `SKILL.md`

Per [`/docs/blueprint/09-skill.md`](../../blueprint/09-skill.md). The frontmatter has the `description` an agent uses to decide whether the skill applies.

### Step 3 — Hand-write `references/examples.md`

Worked end-to-end examples per [`/docs/blueprint/09-skill.md`](../../blueprint/09-skill.md). Quick triage, step through known bug, multi-frame inspection.

### Step 4 — Build the ZIP

`scripts/build-skill.sh`:

```bash
#!/bin/bash
set -e
cargo run --bin generate-skill > /dev/null
cd lazydap.skill && zip -r ../lazydap.skill.tmp . && cd ..
mv lazydap.skill.tmp lazydap.skill
```

Or use a Rust build helper. Output: `lazydap.skill` ZIP at repo root.

### Step 5 — Install for Claude Code

```bash
mkdir -p ~/.claude/skills/
cp lazydap.skill ~/.claude/skills/lazydap.zip
unzip -o ~/.claude/skills/lazydap.zip -d ~/.claude/skills/lazydap/
```

(Or: implement `lazydap install-skill --target claude-code` to do this automatically.)

### Step 6 — Verify with Claude Code

Open Claude Code in any project. Ask:

> "There's a C program at `~/code/planetaryescape/lazydap/examples/c-hello/build/hello`. Run it under lazydap, set a breakpoint at line 6 of main.c, and tell me what the value of `x` is when paused."

Expected: Claude reads `lazydap.skill`, runs `lazydap launch`, `lazydap break`, `lazydap continue --wait`, `lazydap eval "x"`, `lazydap disconnect`. Reports `x = 5`.

If Claude fumbles, the skill needs work. Iterate.

## Success criteria

- `lazydap.skill` ZIP exists, validates as proper ZIP.
- `SKILL.md` and `references/commands.md` content are accurate.
- A test conversation with Claude Code: agent successfully drives a debug session using only the skill.
- No human intervention required during the test conversation (the skill self-explains).

## Files

- `lazydap.skill/SKILL.md` (new)
- `lazydap.skill/references/commands.md` (auto-generated)
- `lazydap.skill/references/examples.md` (new)
- `lazydap.skill/references/error-codes.md` (new — exit codes + error types)
- `lazydap.skill/references/output-schemas.md` (new — JSON output shapes)
- `lazydap.skill` (ZIP, generated)
- `scripts/build-skill.sh` (new)
- CI updated to run `build-skill.sh` and check the ZIP is up to date

## Verify

```bash
./scripts/build-skill.sh
unzip -l lazydap.skill   # confirm contents

# Manual test with Claude Code:
# 1. Install skill into ~/.claude/skills/
# 2. Ask Claude to debug ./examples/c-hello/build/hello
# 3. Agent should drive the session correctly using only `lazydap` subcommands
```

## Depends on

- [`M06-cli-subcommands`](M06-cli-subcommands.md) — full CLI surface.

## Notes

- **The skill is just docs.** Don't add runtime logic to it.
- **Auto-generation matters.** Hand-curated `commands.md` will drift. The build step ensures it's always in sync with clap.
- **Examples should cover the 80% case.** Don't pad with rare scenarios.
- **Test with multiple agents if possible.** Cursor, Copilot, etc. The skill format is portable.
- **After M7, Phase B is done.** Lazydap is a working CLI debugger usable by humans, agents, and scripts. The TUI is next.
