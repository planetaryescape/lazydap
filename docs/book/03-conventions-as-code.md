---
chapter: 3
session_id: WS-3
title: Convention as code
phase: 0
estimated_time_minutes: 45
artifact: All four CI-equivalent commands pass locally and the project ships its first commit
prerequisites:
  - chapter 01 (cargo-workspaces) — workspace exists
  - chapter 02 (tokio-main-clap) — `lazydap-daemon` builds and runs
  - GitHub repo created and configured as `origin`
new_concepts:
  - Convention as code — pinned toolchain, formatter, linter parameters as checked-in files
  - The clippy.toml vs `[workspace.lints.clippy]` split (parameters vs levels)
  - Active toolchain switching (`rust-toolchain.toml`) vs passive (`.nvmrc`)
  - The standard four-step Rust CI pattern (fmt, clippy, check, test)
related_milestone: docs/implementation/00-workspace-setup.md
---

# Chapter 03 — Convention as code

> Session ID: `WS-3` · Phase 0 · ~45 min · [Underlying milestone](../implementation/00-workspace-setup.md)

## What you'll learn

How to pin shared expectations as files that travel with the repo: which toolchain everyone uses, what formatting rules apply, what clippy parameters bind, what license the code is under, what the gitignore excludes, what CI runs on every push. The interesting nuggets you'll pocket along the way: (1) why `clippy.toml` and `[workspace.lints.clippy]` are *distinct* files for *distinct* purposes; (2) why `rust-toolchain.toml` is **active** where `.nvmrc` is **passive**; (3) the standard four-step Rust CI pattern.

This chapter is lighter conceptually than the previous two. Most of it is mechanical setup. The conceptual lift is in the *why* of each file, not the *how*.

## What you'll build

The conventions layer of the project: `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `.gitignore`, `LICENSE-MIT`, `LICENSE-APACHE`, and `.github/workflows/ci.yml`. Plus the project's first commit.

> By the end of this chapter, all four CI-equivalent commands will pass locally:
>
> ```bash
> cargo fmt --all -- --check
> cargo clippy --workspace --all-targets
> cargo check --workspace --all-targets
> cargo test --workspace
> ```
>
> And `git log` will show one commit: the project baseline. That's something you couldn't do at the end of chapter 02. Your code worked, but the project wasn't yet a *project*.

## Before you start

Prior knowledge assumed:

- You've completed [chapter 02](02-tokio-main-clap.md). `cargo run -p lazydap-daemon -- --message hi --count 3` prints `hi` three times.
- You know what `.nvmrc` (or `.python-version`, or `.tool-versions`) does.
- You've checked in a Prettier or ESLint config to a repo.
- You've seen at least one GitHub Actions workflow file.

Setup state required:

- `cargo run -p lazydap-daemon -- --version` prints `lazydap-daemon 0.1.0`.
- A GitHub repo created and added as the `origin` remote (or skip the push step at the end).

If you skipped chapters, see chapters 01 and 02. This one assumes both.

---

## Surface your model first

> 🤔 **Q:** Three sub-questions. Hold them in your head before continuing. (1) What problem does pinning a Node version solve? (2) Why do you check in Prettier and ESLint configs to your repo? (3) Predict the four steps you'd expect a typical Rust CI workflow to run.

<details>
<summary>Click after you've answered</summary>

The first two are usually solid for working web devs:

1. Pinning a Node version eliminates "works on my machine" drift. Everyone on the team and prod build run the same major/minor. The project doesn't depend on whatever Node happens to be installed.
2. Checking in formatter / linter configs makes formatting a property of the *code*, not the developer. New contributors get the rules automatically. Reviewers don't waste cycles on style.

The third is the one that often misses on first try. Common predictions: "type check, build, lint, format." Three of those are right; "type check" is the one that doesn't apply directly.

The standard Rust CI runs four things:

- `cargo fmt --check` for formatter check (read-only)
- `cargo clippy` for lint
- `cargo check` (or `cargo build`) for compile
- `cargo test` to run tests

Why no separate "type check" step: in Rust, the type checker is part of compilation. `cargo check` and `cargo build` both run the borrow checker and type checker as a byproduct of compilation. There's no separate phase. (In TypeScript, by contrast, `tsc --noEmit` is the type check and Vite or webpack is the build; they're separate tools.)

The other surprise for many readers: **tests are part of the standard CI cut**, not a nice-to-have. If the test step isn't there, the CI is incomplete.

</details>

---

## Where chapter 02 left you

You have:

- A workspace with `lazydap-core` (empty library) and `lazydap-daemon` (binary that prints).
- `cargo run -p lazydap-daemon -- --message hi --count 3` prints `hi` three times.
- No conventions files. No CI. No license. No `.gitignore` beyond what `cargo init` produced.

Chapter 03 adds those layers. None of them change runtime behaviour. All of them shape how the project evolves.

---

## Step 1 — `rust-toolchain.toml` (the active one)

Create `rust-toolchain.toml` at the repo root:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

> 🤔 **Q:** This file looks like `.nvmrc`. What's different about it?

<details>
<summary>Click after you've answered</summary>

`.nvmrc` is **passive**: it lives in the repo and waits for *you* to run `nvm use`. Forget to run it, and you're using whatever Node was last active.

`rust-toolchain.toml` is **active**: rustup reads it automatically when you run any cargo command in the directory (or any subdirectory). If the requested toolchain isn't installed, rustup installs it. If it's installed, rustup switches to it. No manual step.

Concretely: with this file in place, run `cd /tmp && rustc --version`, then `cd ~/code/lazydap && rustc --version`. Different toolchains can be active in the two directories, with no `rustup default` switch between them. The active-vs-passive distinction is small, but it removes an entire class of "did you run nvm use" friction from team setups.

`components = ["rustfmt", "clippy"]` ensures the toolchain has both tools installed. Stable toolchains include them by default, but being explicit means you can switch to nightly later (for, say, `cargo expand`) without losing them.

</details>

---

## Step 2 — `rustfmt.toml`

Create `rustfmt.toml`:

```toml
edition = "2024"
max_width = 100
```

That's it. rustfmt's defaults are good. The only overrides worth setting:

- `edition = "2024"`: the formatting rules differ slightly between editions (especially around `let-else` and trailing comma behaviour). Setting this avoids version-skew surprises.
- `max_width = 100`: the default is 100 already. Stating it explicitly documents the intent. You could change it to 120 or 80 here without anyone arguing.

Resist the urge to add 10 more options. Every one of them is a place for a future contributor to disagree. Take the defaults.

---

## Step 3 — `clippy.toml` and the conceptual nugget

Create `clippy.toml`:

```toml
msrv = "1.85"
```

Then look back at the root `Cargo.toml` from chapter 01:

```toml
[workspace.lints.clippy]
unwrap_used = "warn"
panic = "warn"
todo = "warn"
```

You have *two* clippy-related places now. The split looks like duplication; it isn't.

> 🔮 **Predict:** Why are these two separate files? What does each one actually configure?

<details>
<summary>Click after you've predicted</summary>

A common answer: "they're for different scopes" or "one's for the workspace, one's for the crate." That's a partial truth that misses the real distinction.

The split is by **what kind of decision** you're making about a lint:

- **`[workspace.lints.clippy]`** sets the **level** for specific lints. *Should `unwrap_used` be a warning, an error, or allowed?* The values are `"warn"`, `"deny"`, `"allow"`.
- **`clippy.toml`** sets **parameters** for individual lints. *How many arguments before `too-many-arguments` triggers? What's the project's MSRV (Minimum Supported Rust Version)?* The values are numbers, strings, booleans, whatever the lint asks for.

`msrv = "1.85"` in `clippy.toml` tells clippy: "this project promises to work with Rust 1.85 or newer." Clippy then *changes which lints it suggests* based on that. For example, lints that suggest features only available in newer Rust versions get suppressed.

Two questions, two files:

| Question | File | Example |
|---|---|---|
| "Should this lint fire?" / "Should it be a hard error?" | `[workspace.lints.clippy]` (in Cargo.toml) | `unwrap_used = "warn"` |
| "What threshold or parameter does this lint use?" | `clippy.toml` | `too-many-arguments-threshold = 8` |

If you only need levels, you only need `[workspace.lints.clippy]`. If you only need parameters (like MSRV), you only need `clippy.toml`. Most projects need both.

</details>

---

## Step 4 — `.gitignore`

Replace the `cargo init`-generated `.gitignore` with this:

```
/target
.env
.env.*
!.env.example
*.log
.DS_Store
```

Walk through it:

- `/target`: Cargo's build directory. Always ignored. The leading `/` anchors it to the repo root (so a `crates/foo/target` somewhere wouldn't accidentally get ignored).
- `.env` and `.env.*`: environment files with secrets. Never committed.
- `!.env.example`: the negation. `.env.example` *is* committed (it documents what variables are needed); the actual `.env` files are not.
- `*.log`: runtime log files.
- `.DS_Store`: macOS Finder metadata. Harmless but noise.

Lazydap's `.gitignore` will grow over time. Every time CI fails on a file someone shouldn't have committed, the rule goes here.

---

## Step 5 — Licenses

Create both `LICENSE-MIT` and `LICENSE-APACHE`. Standard Rust convention is dual-licensing under both: users pick whichever fits their downstream needs. The `[workspace.package].license = "MIT OR Apache-2.0"` line you wrote in chapter 01 is just metadata; the actual license texts in these two files are what makes that metadata legally meaningful.

You can grab the standard texts from the rust-lang reference:

```bash
curl -sSL https://raw.githubusercontent.com/rust-lang/rust/master/LICENSE-MIT -o LICENSE-MIT
curl -sSL https://raw.githubusercontent.com/rust-lang/rust/master/LICENSE-APACHE -o LICENSE-APACHE
```

Open `LICENSE-MIT` and update the copyright line to:

```
Copyright (c) lazydap contributors
```

(Or your own name, if you prefer to take attribution as a single author. The "contributors" form is a common pattern for projects expected to take outside contributions.)

`LICENSE-APACHE` doesn't need editing. Apache 2.0's text is fixed.

---

## Step 6 — The CI workflow

Create `.github/workflows/ci.yml`. This is the file that GitHub Actions reads to know what to run on push and pull request.

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

env:
  RUSTFLAGS: "-Dwarnings"
  CARGO_TERM_COLOR: always

jobs:
  fmt:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --all -- --check

  clippy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --workspace --all-targets

  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo check --workspace --all-targets

  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace
```

> 🔮 **Predict:** Will these four jobs run in order, or in parallel? What's the practical effect either way?

<details>
<summary>Click after you've predicted</summary>

They run in **parallel**, on four separate runners. There are no `needs:` declarations between them, so GitHub Actions starts them all at once.

Common alternative answer: "in order, cheapest to most expensive, so we fail fast." That logic is right *for a local script*. When a single machine runs all four, you'd order them fmt → clippy → check → test so a fast failure aborts before the slow steps. But CI on GitHub Actions runs each job on its own machine; parallel is faster than sequential as long as you're willing to use four runners' worth of compute per push.

You still get fail-fast behaviour. Whichever job fails first reports failure within seconds of starting, and you can see the failed job before the others finish. The `concurrency:` block at the top adds another fail-fast: if you push a new commit while CI is running on an earlier commit of the same PR, the older run is cancelled.

For *local* sequential running, you'd use a Makefile or a justfile with a `check` target that runs them in order. Lazydap doesn't have one yet; chapter 03 keeps the project surface minimal.

</details>

Walk the YAML once more:

- `concurrency:` kills stale runs on the same branch when a new push lands. Saves runner time.
- `env: RUSTFLAGS: "-Dwarnings"` turns any compiler warning into a hard error. Lazydap's policy: warnings don't accumulate.
- `Swatinem/rust-cache@v2` caches the `target/` directory and Cargo registry between runs. First-run is slow; subsequent runs are fast.
- `dtolnay/rust-toolchain@stable` installs the Rust toolchain on the runner. The action respects your `rust-toolchain.toml` if present.

This is the standard shape. Most production Rust projects use a workflow that's structurally identical to this one.

---

## Step 7 — Run the four steps locally

Before you push and let CI run for real, run all four locally:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo check --workspace --all-targets
cargo test --workspace
```

> 🔮 **Predict:** Will all four pass? If one fails, which is most likely?

<details>
<summary>Click after you've predicted</summary>

`cargo fmt --check` is the most likely to surprise you on first run. A common cause: files without trailing newlines. If `crates/core/src/lib.rs` is zero bytes from chapter 01, rustfmt will reject it.

The expected sequence:

```
$ cargo fmt --all -- --check
Diff in .../crates/core/src/lib.rs at line 1:
+
```

That `+` at line 1 is rustfmt saying "the file should have at least a newline." Fix:

```bash
cargo fmt --all
```

Now re-run the check:

```bash
$ cargo fmt --all -- --check
$  # (no output, exit 0)
```

The other three should pass on a clean checkout:

- `cargo clippy --workspace --all-targets`: no clippy warnings on the small surface so far.
- `cargo check --workspace --all-targets`: compiles fine.
- `cargo test --workspace`: runs zero tests but exits cleanly. The output will mention `Doc-tests lazydap_core`. `cargo test` runs code in doc comments as tests automatically. You don't have any docs yet; later chapters will.

The empty-file-needs-newline gotcha is worth pocketing. It's a common CI-vs-local mismatch. Your editor probably auto-adds a final newline; a programmatically-created empty file won't have one. Always end files with a newline.

</details>

---

## Step 8 — The first commit

Stage everything and commit:

```bash
git add .
git status
```

You should see:

- `.github/workflows/ci.yml` (new)
- `.gitignore` (modified or new depending on your starting state)
- `Cargo.toml` (modified, the workspace from chapter 01)
- `Cargo.lock` (new)
- `LICENSE-APACHE` (new)
- `LICENSE-MIT` (new)
- `clippy.toml` (new)
- `crates/core/Cargo.toml` (new)
- `crates/core/src/lib.rs` (new)
- `crates/daemon/Cargo.toml` (new)
- `crates/daemon/src/main.rs` (new)
- `rust-toolchain.toml` (new)
- `rustfmt.toml` (new)

And any documentation files you've already added (blueprint, implementation, etc., if your repo has them).

Commit:

```bash
git commit -m "chore: initial workspace scaffold"
git push
```

For a project's *first* commit, one big "establish baseline" commit is conventional. Splitting historical docs from new code at this stage is churn for no benefit. Subsequent commits should be small and focused. Chapter 04's first move will be M0-related, scoped to one milestone.

If you have GPG or SSH commit signing set up, the commit will be signed automatically and show up as `Verified` on GitHub. Lazydap signs its commits with ED25519 keys. (Optional but recommended for any project that aims to be public.)

Open the repo on GitHub. Watch the four CI jobs run. They should all turn green within ~3 minutes (longer on the very first run because the cache is empty).

---

## Try it yourself

> 🛠️ **Your turn:** Add `.cargo/config.toml` with this content:
>
> ```toml
> [build]
> rustflags = ["-Dwarnings"]
> ```
>
> Then explain (in your head) what this does, and how it interacts with the `RUSTFLAGS` environment variable in `.github/workflows/ci.yml`.

<details>
<summary>Click for the answer</summary>

`.cargo/config.toml` is read by Cargo whenever you run a command in this directory. The `[build] rustflags` setting applies the same `-Dwarnings` rule **locally** that CI applies in the cloud. Now `cargo build` locally fails on warnings, matching CI behaviour.

The interaction with the `RUSTFLAGS` env var in CI: the env var takes precedence when set. Setting both is belt-and-braces. The env var ensures CI is strict, the config file ensures local dev is also strict. They don't conflict.

The trade-off to think about: if you commit `[build] rustflags = ["-Dwarnings"]` to your repo, every contributor gets a local environment where warnings are errors. Some projects prefer to keep warnings warning-only locally (so you can keep iterating in the middle of a refactor) and leave the strict treatment for CI. Lazydap chose to keep both strict. The team is small and the codebase is young.

You can either keep `.cargo/config.toml` or revert it. The chapter exercise was the reasoning, not the file.

</details>

---

## Compiler conversation

There's no compiler error to walk through this chapter. The conventions files are mostly TOML and YAML, and the only "error" you'll likely hit is the rustfmt empty-file complaint from step 7. That itself is a good walkthrough of "here's the kind of thing CI catches that local trust-yourself misses."

If you want to deliberately break something to feel CI catch it: add `let x = 1;` (an unused variable) to `crates/core/src/lib.rs`, run `cargo build`, and watch the warning. Then run with `-Dwarnings`:

```bash
RUSTFLAGS="-Dwarnings" cargo build
```

The same warning is now a hard error. This is exactly the upgrade the CI's `RUSTFLAGS: "-Dwarnings"` env var applies. Restore `lib.rs` to empty.

---

## What you can run now

```bash
cargo fmt --all -- --check && \
cargo clippy --workspace --all-targets && \
cargo check --workspace --all-targets && \
cargo test --workspace
```

All four pass. Your local environment matches what CI runs. Push, and CI passes too.

```bash
git log --oneline
```

Output (your hash will differ):

```
6a06e68 chore: initial workspace scaffold
```

The project is committed. Workspace, daemon, conventions, license, CI, all in one baseline commit.

**Ladder check.** Chapter 01 gave you a workspace that builds. Chapter 02 gave you a binary that runs and prints. Chapter 03 turns those two artifacts into a *project*: pinned toolchain, formatter rules, clippy rules, license, CI on every push. The next time someone (or a coding agent) clones this repo, `cargo build --workspace` will use the exact same toolchain you used, format checks the same way, run the same lints, run the same tests. The "works on my machine" failure mode is closed.

Forward look: chapter 04 is the first *substantial* Rust session: `tokio::process::Command` and spawning codelldb, the first external debugger lazydap will talk to. The conceptual stake jumps significantly. The slow workspace setup is over; the real Rust learning starts there.

---

## Teach-back

Before moving on, answer these in your own words.

> 📣 **Q1:** What's the difference between `clippy.toml` and `[workspace.lints.clippy]`? Give an example of something that goes in each.

> 📣 **Q2:** What does `rust-toolchain.toml` do that `.nvmrc` doesn't? In one sentence.

> 📣 **Q3:** A new contributor clones the repo and runs `cargo build`. Walk through what each of these files contributes to their experience: `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `Cargo.toml` (workspace).

> 📣 **Q4:** The CI runs four jobs. Why those four, in any order? What does each one catch that the others don't?

---

## Pain anchors covered

| New construct | The pain it solves | In which language |
|---|---|---|
| `rust-toolchain.toml` (active) | "Did you run `nvm use`?" / "Wrong Node version" / "Different team members on different toolchains" | JavaScript |
| `rustfmt.toml` checked in | Style arguments in code review | Any language without a built-in formatter |
| `clippy.toml` separate from `[workspace.lints.clippy]` | Conflating "is this a warning?" with "what threshold does this lint use?". They answer different questions | Rust ergonomics |
| Standard CI workflow | Manual `make check` rituals before push, forgetting to lint, "it built locally" | C, JavaScript, anywhere |
| Dual `LICENSE-MIT` + `LICENSE-APACHE` | "What license is this code under?" / "Can I use this in my MIT project?". Surfaces the answer in the repo | Open source convention |
| `RUSTFLAGS: -Dwarnings` in CI | Warnings accumulating until "let's deal with these later" becomes "let's deal with 800 of these never" | Any language with warnings |

---

## See also

- ← [Chapter 02: Async main and clap](02-tokio-main-clap.md)
- → [Chapter 04: Hello, adapter](04-hello-adapter.md)
- [Underlying milestone: workspace setup](../implementation/00-workspace-setup.md)
- [rustup book: rust-toolchain.toml](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file)
- [rustfmt configuration reference](https://rust-lang.github.io/rustfmt/)
- [clippy configuration reference](https://doc.rust-lang.org/clippy/configuration.html)
- [The Cargo Book: lints](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section)
- Anchor codebase: `mxr/.github/workflows/ci.yml` for the same shape at production scale
