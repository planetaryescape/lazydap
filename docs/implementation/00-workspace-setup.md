# 00 — Workspace setup

Done before M0. Sets up the Cargo workspace, CI, and conventions so every milestone after this slots into a stable structure.

## What

Empty Cargo workspace at repo root with placeholder crate `lazydap-core`. CI pipeline runs `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt --check`. Working `cargo run --bin lazydap -- --help` (returns "lazydap pre-alpha", exit 0).

## Why

So M0 starts with a working build. So we don't conflate "set up Rust project" with "talk to codelldb" — that's two unknowns at once.

## How

### Step 1 — Initialise

```bash
cd ~/code/planetaryescape/lazydap
cargo init --bin --name lazydap
```

Edit `Cargo.toml` to be a workspace:

```toml
[workspace]
resolver = "2"
members = [
    "crates/core",
]

[workspace.package]
version = "0.0.0"
edition = "2021"
license = "MIT OR Apache-2.0"
authors = ["Bhekani Khumalo"]
repository = "https://github.com/planetaryescape/lazydap"
rust-version = "1.75"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
thiserror = "1"
uuid = { version = "1", features = ["v7", "serde"] }
clap = { version = "4", features = ["derive", "wrap_help"] }
toml = "0.8"
async-trait = "0.1"
```

The root `Cargo.toml` is workspace-only (no `[package]` section). Move the binary into a daemon crate later. For now, simplest path:

```bash
mkdir -p crates/core
```

`crates/core/Cargo.toml`:

```toml
[package]
name = "lazydap-core"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true

[dependencies]
serde.workspace = true
thiserror.workspace = true
uuid.workspace = true
```

`crates/core/src/lib.rs`:

```rust
//! lazydap core types. Zero I/O. See ARCHITECTURE.md.

#![warn(clippy::pedantic)]

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
```

### Step 2 — Daemon binary skeleton

```bash
mkdir -p crates/daemon/src
```

`crates/daemon/Cargo.toml`:

```toml
[package]
name = "lazydap-daemon"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "lazydap"
path = "src/main.rs"

[dependencies]
lazydap-core = { path = "../core" }
clap.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
```

Add `crates/daemon` to workspace members.

`crates/daemon/src/main.rs`:

```rust
use clap::Parser;

#[derive(Parser)]
#[command(name = "lazydap", version, about = "Scriptable terminal-first debugger")]
struct Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let _cli = Cli::parse();
    println!("lazydap pre-alpha — see https://github.com/planetaryescape/lazydap");
    Ok(())
}
```

Add `anyhow = "1"` to workspace deps and the daemon crate. (Anyhow only in binary crates; libraries use `thiserror`.)

### Step 3 — Verify

```bash
cargo build
cargo run --bin lazydap -- --help
cargo test --workspace
```

All three pass.

### Step 4 — CI

`.github/workflows/ci.yml`:

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --workspace --all-targets
      - run: cargo test --workspace
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo fmt --all --check
```

Push, confirm green.

### Step 5 — Conventions files

`rustfmt.toml`:

```toml
edition = "2021"
max_width = 100
```

`clippy.toml`:

```toml
# Pedantic enabled per-crate via #![warn(clippy::pedantic)] in lib.rs files.
# Some pedantic lints suppressed because their fixes hurt readability.
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
```

### Step 6 — License files

```bash
curl -sSL https://raw.githubusercontent.com/rust-lang/rust/master/LICENSE-MIT -o LICENSE-MIT
curl -sSL https://raw.githubusercontent.com/rust-lang/rust/master/LICENSE-APACHE -o LICENSE-APACHE
```

Update author + year in MIT.

### Step 7 — gitignore

```
target/
*.swp
.DS_Store
.lazydap/
```

(`.lazydap/` ignored by default; users opt in by removing.)

### Step 8 — First commit

```bash
git add .
git commit -m "chore: initial workspace skeleton"
git push
```

## Success criteria

- `cargo build --workspace` passes
- `cargo test --workspace` passes (no tests yet, but the runner runs)
- `cargo clippy --workspace --all-targets -- -D warnings` passes
- `cargo fmt --check` passes
- `cargo run --bin lazydap -- --help` prints lazydap version + about line
- CI green on push to main

## Files

- `Cargo.toml` (workspace)
- `crates/core/Cargo.toml`, `crates/core/src/lib.rs`
- `crates/daemon/Cargo.toml`, `crates/daemon/src/main.rs`
- `rustfmt.toml`, `clippy.toml`, `rust-toolchain.toml`
- `LICENSE-MIT`, `LICENSE-APACHE`
- `.gitignore`
- `.github/workflows/ci.yml`

## Verify

```bash
cargo build && cargo test && cargo clippy && cargo fmt --check
cargo run --bin lazydap
# Output: "lazydap pre-alpha — see https://github.com/planetaryescape/lazydap"
```

## Depends on

Nothing. This is the prerequisite for everything.

## Notes

- Don't add crates beyond `core` and `daemon` here. Each milestone adds the crates it needs.
- Don't add `tokio::main` macro features beyond `full` — premature optimisation.
- `pedantic` clippy is on by default at lib level. If a lint is genuinely bad, suppress with a comment, not by removing pedantic globally.
