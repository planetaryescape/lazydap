---
chapter: 1
session_id: WS-1
title: Cargo workspaces
phase: 0
estimated_time_minutes: 60
artifact: A virtual Cargo workspace with one member crate that builds cleanly via `cargo build --workspace`
prerequisites:
  - Rust toolchain installed (`rustc --version` works)
  - Git installed
  - You can run `cargo init` in an empty directory
new_concepts:
  - Cargo workspaces — root-level `[workspace]` declaration
  - Workspace inheritance — opt-in field-by-field via `*.workspace = true`
  - Virtual vs hybrid workspaces — root has no `[package]` of its own
related_milestone: docs/implementation/00-workspace-setup.md
---

# Chapter 01 — Cargo workspaces

> Session ID: `WS-1` · Phase 0 · ~60 min · [Underlying milestone](../implementation/00-workspace-setup.md)

## What you'll learn

The shape of a Cargo workspace: the four pillars at the root (`[workspace]`, `[workspace.package]`, `[workspace.dependencies]`, `[workspace.lints]`), and how member crates opt into inheritance one field at a time. You'll also learn the difference between a *virtual* workspace (no root package) and a *hybrid* workspace (root is also a crate).

This chapter has nothing to say about async, traits, lifetimes, or borrowing. Those are deferred. The one new concept is workspace structure.

## What you'll build

A bare Cargo workspace with one member crate (`lazydap-core`) that compiles to an empty library. Nothing runs or prints yet. That's chapter 02.

> By the end of this chapter, running `cargo build --workspace` will compile your member crate using metadata it inherits from the root, and `cargo metadata --format-version 1 | jq '.workspace_members'` will list `lazydap-core` as a workspace member. That's something you couldn't do before chapter 01.

## Before you start

Prior knowledge assumed:

- You've used `npm` or `pnpm` or `bun` for at least one project. You know what `package.json` is.
- You've heard of monorepos. (You don't have to have built one.)
- You can read TOML.

Setup state required:

- `rustc --version` should print a version >= `1.85` (chapter 03 pins this; for now anything recent enough works)
- `cargo --version` should print a matching version
- An empty directory you can work in (e.g., `~/code/lazydap`)

If you skipped chapters, this is chapter 01. There's nothing to skip from. Start here.

---

## Surface your model first

> 🤔 **Q:** How do you think a Cargo workspace differs from just having one `Cargo.toml` at the root? What problem do you think it solves? If you're an npm person, where do you think the analogy lands?

Pause and answer in your head before continuing.

<details>
<summary>Click after you've answered</summary>

A common answer: "`Cargo.toml` is `package.json`, `Cargo.lock` is `package-lock.json`, so a workspace is probably just `cargo init` plus some convention on top." That's a half-truth. The file analogies are right, but they describe a *single-package* setup. A Cargo workspace is the **monorepo** concept.

The right npm-world parallel is **npm/yarn/pnpm workspaces** (or Turborepo, or Nx): multiple sub-packages under one root with a shared lockfile. Specifically:

- A regular Cargo package: one `Cargo.toml`, one library or binary. ≈ `npm init` for a single project.
- A Cargo workspace: a root `Cargo.toml` with `[workspace]` and `members = [...]`. Each member has its **own** `Cargo.toml`. They share `Cargo.lock` and `target/`.

Cargo workspaces add one feature that npm-classic doesn't fully match: **opt-in field-by-field inheritance**. You declare `[workspace.package]` and `[workspace.dependencies]` once at the root, and each member opts in one field at a time. The closest npm-world equivalent is pnpm's `catalog:` feature.

Hold that mental model (provider/subscriber, opted into per field) through the rest of the chapter.

</details>

---

## The pain workspaces solve

If you're learning C in parallel (a lot of readers are), you've probably already felt the C side of this. There's no canonical answer to "how do I depend on someone else's library?" The answer is some mix of `make`, `cmake`, system package managers, vendored copies, header search paths, linker flags, and prayer. Different projects pick different combinations. The "what version of zlib am I using" question can be genuinely unanswerable.

| Cargo feature | The pain it fixes | Where you've felt it |
|---|---|---|
| `Cargo.toml` declares deps | C's "what build system, what package manager, where do dependencies come from, what version" sprawl | C |
| Workspace shared `Cargo.lock` | npm monorepo's per-package `node_modules` duplication and version-skew chaos | JavaScript |
| `[workspace.package]` inheritance | "Bumping the version requires editing 12 files and forgetting two of them" | Any monorepo, any language |
| `[workspace.dependencies]` | Per-package dep versions drifting; security audits failing because crate X is on three different versions across the repo | Any monorepo |

Workspaces are not a Rust language feature. They're a *Cargo* feature. The Rust compiler doesn't know workspaces exist. But workspaces are how every multi-crate Rust project is structured, so you'll touch them every day.

---

## The starting state

Open a terminal in your project directory.

```bash
cargo init --bin --name lazydap
```

This produces:

```
.
├── Cargo.toml
├── .gitignore
└── src
    └── main.rs
```

Look at the generated `Cargo.toml`:

```toml
[package]
name = "lazydap"
version = "0.1.0"
edition = "2021"

[dependencies]
```

> 🔮 **Predict:** Is this a workspace? If you ran `cargo build` right now, what would build?

<details>
<summary>Click after you've predicted</summary>

It is **not** a workspace. It's a single-package project. There's no `[workspace]` table. `cargo build` would compile `src/main.rs` into a binary named `lazydap`.

That's not what you want. You want a *virtual* workspace: a root that's only a workspace, with no package of its own. The root coordinates; the member crates do the actual work.

</details>

---

## Step 1 — Delete the root crate

Because you're building a virtual workspace, the root will not be a crate. Delete the `src/` directory.

```bash
rm -rf src
```

This is the first decision: virtual vs hybrid.

- **Virtual workspace**: root has `[workspace]` only, no `[package]`. The root isn't a crate; it's a coordinator. Examples: most multi-crate Rust projects.
- **Hybrid workspace**: root has both `[workspace]` and `[package]`. The root is itself a member of its own workspace. Examples: many single-binary projects with helper crates split out as workspace members.

Pick one and commit to it. Lazydap will be virtual; every concrete crate gets its own directory under `crates/`. (Your anchor codebase, mxr, is hybrid. Read its `Cargo.toml` side-by-side as you go through this chapter and notice the difference.)

> 🤔 **Q:** What's the trade-off? Why would anyone pick hybrid over virtual?

<details>
<summary>Click after you've answered</summary>

Hybrid is convenient when there's exactly one binary and a few helper libraries. The root *is* the project, and splitting it artificially adds ceremony. Virtual is cleaner when there will be multiple crates of equal weight (a daemon, a TUI, a protocol crate, a core types crate, several adapter crates). Lazydap is the second case. Pick the shape that matches the eventual crate population, not the current one.

</details>

---

## Step 2 — The four pillars

Rewrite `Cargo.toml` as a workspace manifest. There are four pillar tables you'll see in nearly every workspace root:

```toml
[workspace]
resolver = "2"
members = ["crates/core"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
rust-version = "1.85"
repository = "https://github.com/planetaryescape/lazydap"
homepage = "https://github.com/planetaryescape/lazydap"

[workspace.lints.rust]
unsafe_code = "deny"
unused_must_use = "deny"

[workspace.lints.clippy]
unwrap_used = "warn"
panic = "warn"
todo = "warn"

[workspace.dependencies]
```

Walk through the four pillars one at a time.

### Pillar 1: `[workspace]`

```toml
[workspace]
resolver = "2"
members = ["crates/core"]
```

- `resolver = "2"` picks Cargo's feature resolver version. v2 is the modern one (since Rust 1.51). Always set this explicitly; relying on the default is a common source of surprising feature-unification bugs.
- `members` lists the member crates, by directory path. Glob patterns (`"crates/*"`) work too. Lazydap spells out members explicitly until there are enough crates that a glob is worth it.

### Pillar 2: `[workspace.package]`

```toml
[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
...
```

This is the *provider* of metadata. None of these fields *applies* to anything yet. They're just sitting at the root waiting for member crates to opt into them. Field-by-field. One at a time. (If you've used pnpm `catalog:`, the shape will feel familiar.)

### Pillar 3: `[workspace.dependencies]`

```toml
[workspace.dependencies]
```

Empty for now. Later chapters fill this in (chapter 02 adds `tokio` and `clap`). Same pattern as `[workspace.package]`: declare once at the root, opt in per member.

### Pillar 4: `[workspace.lints]`

```toml
[workspace.lints.rust]
unsafe_code = "deny"
unused_must_use = "deny"

[workspace.lints.clippy]
unwrap_used = "warn"
panic = "warn"
todo = "warn"
```

Set lint *levels* (warn / deny / allow) for the workspace once. Member crates opt in with `[lints] workspace = true` and inherit the whole table.

The `unsafe_code = "deny"` line means: any crate that opts into workspace lints will fail to compile if it contains an `unsafe` block. Lazydap has no need for `unsafe`; making the rule explicit prevents accidental introduction.

> 🔮 **Predict:** Cargo has a *separate* file called `clippy.toml`. What's it for, and why isn't it the same as `[workspace.lints.clippy]` here?

<details>
<summary>Click after you've predicted</summary>

Chapter 03 covers this in depth. The short answer: `[workspace.lints.clippy]` is for lint **levels** (warn / deny / allow). `clippy.toml` is for lint **parameters** (e.g., "how many arguments before `too-many-arguments` triggers?", "what's the MSRV?"). Two distinct files because they answer two distinct questions about the same lints. You'll set up `clippy.toml` in chapter 03.

</details>

---

## Step 3 — The first member crate

Create the directory and files.

```bash
mkdir -p crates/core/src
touch crates/core/src/lib.rs
```

Create `crates/core/Cargo.toml`:

```toml
[package]
name = "lazydap-core"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
repository.workspace = true
homepage.workspace = true
publish = false

[lints]
workspace = true
```

Read this carefully. A field can appear in exactly two ways:

- `version.workspace = true` means "inherit this field from the workspace root."
- `publish = false` is a literal value, not inherited.

Notice what's *not* there:

- No `[workspace]` table. This file is a member, not a workspace root.
- No version string. The version comes from the root, by inheritance.
- No `authors`. (You can inherit `authors.workspace = true` if you set `[workspace.package].authors`. Lazydap doesn't bother.)

> 🔮 **Predict:** What happens if you write `version = "0.1.0"` (a literal) in `crates/core/Cargo.toml` instead of `version.workspace = true`?

<details>
<summary>Click after you've predicted</summary>

It works. The crate just doesn't inherit. It uses the literal value you wrote. Inheritance is opt-in field-by-field; opting out is the default.

The trap: you now have *two* sources of truth for the version. Bumping the workspace root won't bump this crate. Six months from now you'll release, run `cargo publish`, and watch one crate go out at `0.2.0` and another at `0.1.0` because someone forgot. The discipline is "opt in by default; opt out only with a written reason."

</details>

> 🔮 **Predict:** What does `publish = false` do? Why set it?

<details>
<summary>Click after you've predicted</summary>

`publish = false` tells Cargo: this crate is private to the workspace. `cargo publish` will refuse to upload it to crates.io. If you're building a workspace where some crates are public (`lazydap-core`, eventually) and some are internal (a `lazydap-test-utils` would qualify), you mark the internal ones with `publish = false` to prevent accidents.

For now, every lazydap crate gets `publish = false` until you're ready to release v0.1 (chapter 15-something, far away). Setting it preemptively is cheap insurance.

</details>

---

## Step 4 — Make the empty crate compile

`crates/core/src/lib.rs` is currently zero bytes. Try to build:

```bash
cargo build --workspace
```

> 🔮 **Predict:** Will this build? It's a library file with no contents.

<details>
<summary>Click after you've predicted</summary>

Most readers predict failure: "an empty file can't be a valid module, it has nothing in it."

It compiles. An empty `lib.rs` is a valid library with zero public items. The output is an empty `.rlib`. Rust libraries don't need a "main" entry point, and they don't need any items to be valid. They're just a namespace that happens to be empty.

The companion fact, which is the trap: an empty `main.rs` does *not* compile. A binary needs a `fn main()`. The linker will refuse: "main function not found." Empty lib OK, empty bin no.

You'll see `crates/daemon/src/main.rs` get a real `fn main` in chapter 02 for exactly this reason.

</details>

Run it:

```bash
cargo build --workspace
```

Expected output:

```
   Compiling lazydap-core v0.1.0 (.../lazydap/crates/core)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.03s
```

The `0.1.0` in the build output came from `[workspace.package].version` at the root, flowed through `version.workspace = true` in the member's `Cargo.toml`, and showed up in the build summary. End-to-end inheritance is working.

If you got something different, common causes:

- "could not find `Cargo.toml`": you're not in the workspace root. `cd` to the directory containing the workspace `Cargo.toml`.
- "failed to load manifest for workspace member `crates/core`": typo in `members`, the directory doesn't exist, or `crates/core/Cargo.toml` is missing.
- "the field `version` is required": you have a member crate but `[workspace.package]` doesn't define `version`, or you forgot `version.workspace = true` in the member.

---

## Try it yourself

> 🛠️ **Your turn:** Add a second member crate, `lazydap-protocol`, that inherits from the workspace exactly the same way `lazydap-core` does. Don't write any code in it. Just the manifest and an empty `lib.rs`.

Steps:

1. `mkdir -p crates/protocol/src`
2. Create `crates/protocol/src/lib.rs` (empty file).
3. Create `crates/protocol/Cargo.toml` matching the shape of `crates/core/Cargo.toml`, but with `name = "lazydap-protocol"`.
4. Add `"crates/protocol"` to the workspace's `members` array.
5. Run `cargo build --workspace`.

Expected output:

```
   Compiling lazydap-core v0.1.0 (.../lazydap/crates/core)
   Compiling lazydap-protocol v0.1.0 (.../lazydap/crates/protocol)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s
```

If you got something different, common causes:

- "no targets specified in the manifest": you're missing `crates/protocol/src/lib.rs`. The default crate target is determined by the presence of `src/lib.rs` (library) or `src/main.rs` (binary). If neither exists, Cargo doesn't know what to build.
- "missing field `name`": every `[package]` block needs a literal `name`. There's no `name.workspace = true`; the name *must* be unique per crate.

When you've confirmed it builds, **delete the protocol crate**. You don't need it yet. The exercise was the build, not the artifact. (If you want to keep it for later chapters, leave it. It gets fleshed out around chapter 14.)

---

## Compiler conversation

Try this deliberate mistake. In `crates/core/Cargo.toml`, change:

```toml
version.workspace = true
```

to:

```toml
version.workspace = true
nonexistent_field.workspace = true
```

Run `cargo build --workspace`. Read the error.

```
error: failed to parse manifest at `.../lazydap/crates/core/Cargo.toml`

Caused by:
  `nonexistent_field` is not a valid field for inheritance from workspace
```

What this tells you: only specific fields can be inherited (`version`, `authors`, `description`, `documentation`, `readme`, `homepage`, `repository`, `license`, `license-file`, `keywords`, `categories`, `publish`, `edition`, `rust-version`). The set is fixed by Cargo. If you need to share something not on the list, you can't use this mechanism. You'd have to copy the literal value into each crate.

Restore the file. Save.

---

## What you can run now

```bash
cargo build --workspace
```

Output:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.01s
```

```bash
cargo metadata --format-version 1 | jq '.workspace_members'
```

Output (your hash will differ):

```
[
  "lazydap-core 0.1.0 (path+file:///.../lazydap/crates/core)"
]
```

That `0.1.0` flowed from the root manifest, through inheritance, into the metadata Cargo reports about your workspace. Workspace structure is real and live.

**Ladder check.** This is chapter 01, so there's no previous chapter to ladder back to. The starting position was an empty directory; the ending position is a working workspace with one member crate that compiles. The rest of the project piles crates into this skeleton.

Forward look: chapter 02 adds a *binary* crate (the daemon) that takes command-line arguments and prints them. That's the first time you'll see something user-visible run.

---

## Teach-back

Before moving on, answer these in your own words. If you can't, re-read the relevant section.

> 📣 **Q1:** Explain Cargo workspaces to a colleague who knows JavaScript but not Rust. What's the workspace, what's a member crate, and how does inheritance work?

> 📣 **Q2:** What's the difference between a virtual and a hybrid workspace? When would you pick each?

> 📣 **Q3:** You add `description = "lazydap core types"` to `[workspace.package]` and want the description to show up on `crates/core`. What do you write in `crates/core/Cargo.toml`?

---

## Pain anchors covered

| New construct | The pain it solves | In which language |
|---|---|---|
| `[workspace]` + `Cargo.lock` | C's "what build system, what package manager, what version?" sprawl | C |
| `[workspace]` + per-member `Cargo.toml` | npm monorepos: per-package `node_modules`, version skew across packages | JavaScript |
| `[workspace.package]` + `field.workspace = true` | Bumping a version means editing every package and missing one | Any monorepo |
| Virtual workspace | "My single-binary project doesn't need a root crate cluttering the layout" | Rust convention |

---

## See also

- → [Chapter 02: Async main and clap](02-tokio-main-clap.md)
- [Underlying milestone: workspace setup](../implementation/00-workspace-setup.md)
- [Cargo book: workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
- [Cargo book: workspace inheritance](https://doc.rust-lang.org/cargo/reference/workspaces.html#the-package-table)
- Anchor codebase: `mxr/Cargo.toml` (hybrid workspace; read it side-by-side and notice the difference)
