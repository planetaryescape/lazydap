---
chapter: 2
session_id: WS-2
title: Async main and clap
phase: 0
estimated_time_minutes: 75
artifact: A binary `lazydap-daemon` that parses `--message` and `--count` arguments and prints
prerequisites:
  - chapter 01 (cargo-workspaces) — workspace exists with `lazydap-core`
  - tokio installed via workspace dependency (added in this chapter)
new_concepts:
  - Rust attribute and derive macros — compile-time source rewrite (not runtime decorator)
  - `#[tokio::main]` and what `async fn main` actually compiles to (gloss only — deep async semantics deferred)
  - `#[derive(Parser)]` from clap and how a struct definition becomes a CLI
related_milestone: docs/implementation/00-workspace-setup.md
---

# Chapter 02 — Async main and clap

> Session ID: `WS-2` · Phase 0 · ~75 min · [Underlying milestone](../implementation/00-workspace-setup.md)

## What you'll learn

How Rust's annotation macros (`#[tokio::main]`, `#[derive(Parser)]`) generate code at compile time, and why this is fundamentally different from runtime decorators in TypeScript or Python. The unifying mental model: **Rust attribute macros are like Babel plugins or TypeScript transformers**, not runtime higher-order functions.

You'll touch async syntax (`async fn`, the `#[tokio::main]` macro) but you won't learn what async actually compiles to. That's a load multiplier deferred to a later session.

## What you'll build

A second member crate, `lazydap-daemon`, with a binary that parses two command-line arguments (`--message` and `--count`) and prints the message that many times.

> By the end of this chapter, running `cargo run -p lazydap-daemon -- --message hi --count 3` will print `hi` three times. That's something you couldn't do at the end of chapter 01, when you had only an empty library that compiled.

## Before you start

Prior knowledge assumed:

- You've completed [chapter 01](01-cargo-workspaces.md). The workspace exists with `lazydap-core` as a member.
- You've used decorators in TypeScript, Python, or Java, *or* you've never seen them. Both are fine. The chapter calibrates either way.
- You know what a CLI argument parser does (you've used `argparse`, `commander`, `yargs`, `clap`, `cobra`, etc., at least once).

Setup state required:

- `cargo build --workspace` from chapter 01 should finish with one crate (`lazydap-core`) compiled.
- A clean working tree (so you can see this chapter's diff in isolation).

If you skipped chapter 01, you don't have a workspace. Go back. Chapter 02 adds a *second* crate, which requires a workspace to add it to.

---

## Surface your model first

> 🤔 **Q:** How do you think `#[tokio::main]` works? If you've used decorators in TypeScript or Python, anchor on that. What do you think the macro does to the function below it?

```rust
#[tokio::main]
async fn main() {
    println!("hello");
}
```

Pause and answer in your head.

<details>
<summary>Click after you've answered</summary>

A common answer: "Decorators wrap a function with another function that adds behaviour. So `#[tokio::main]` probably wraps `main` with code that sets up an async runtime before running the body." That's *half right*.

The correct half: yes, the macro produces wrapping code, and yes, the result enables async.

The half that needs correcting: TypeScript and Python decorators run at **runtime**. They're regular functions that take a function (or class) as input and return a (possibly modified) function. The decorator and the original function both exist at runtime; the wrapping happens when the decorator runs.

Rust attribute macros run at **compile time**. They take source code as a token stream / AST and emit *different* source code, before the compiler ever sees the original. The compiler then type-checks the *output*, not the input.

The better mental model: **Rust attribute macros are Babel plugins or TypeScript transformers**. They rewrite source. They're not higher-order functions.

Three implications fall out of this:

1. Macros can see the **syntax** of what they decorate, including markers like `async`, `pub`, generic parameters. A runtime decorator can only see the runtime value (a function object). It can't see whether the source had the word `async` in it without reflection.
2. **Compile errors can come from the *expanded* code.** The error message points at lines you didn't write. This is a Rust learning rite of passage. When that happens, `cargo expand` shows the post-macro source.
3. **Zero runtime overhead.** The annotation disappears entirely at compile time. There's no decorator object hanging around.

Hold this distinction. It's the unifying concept of the chapter.

</details>

---

## Where chapter 01 left you

Recap before adding anything new: you have a virtual workspace with one member crate, `lazydap-core`, that's an empty library. `cargo build --workspace` succeeds and produces an empty `.rlib`. The root `Cargo.toml` has the four pillars set up; `[workspace.dependencies]` is empty.

Chapter 02 adds:

1. A second member crate, `lazydap-daemon`, in `crates/daemon/`.
2. Two new entries in `[workspace.dependencies]`: `tokio` and `clap`.
3. A `main.rs` that uses both.

By the end you'll have something runnable. Your first artifact-that-prints.

---

## Step 1 — A failing first attempt

Set up the daemon directory.

```bash
mkdir -p crates/daemon/src
```

Add the daemon to the workspace `members` array in the root `Cargo.toml`:

```toml
members = ["crates/core", "crates/daemon"]
```

Create `crates/daemon/Cargo.toml`:

```toml
[package]
name = "lazydap-daemon"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish = false

[lints]
workspace = true

[[bin]]
name = "lazydap-daemon"
path = "src/main.rs"

[dependencies]
```

The `[[bin]]` table is new. You didn't have one in `lazydap-core` because that crate was a library. A binary crate needs to declare what binary it produces. `name` is what you'll type to `cargo run -p`; `path` is where the entry point lives. (Cargo will infer this from `src/main.rs` automatically, but writing it out is clearer.)

Create `crates/daemon/src/main.rs`, deliberately broken:

```rust
async fn main() {
    println!("hello from lazydap-daemon");
}
```

> 🔮 **Predict:** Will this compile? If yes, what does it do? If no, what's the error?

<details>
<summary>Click after you've predicted</summary>

It will not compile. Most readers predict "compiler will reject with a hard error saying main can't be async". That's exactly right. The full error:

```
error[E0752]: `main` function is not allowed to be `async`
 --> crates/daemon/src/main.rs:1:1
  |
1 | async fn main() {
  | ^^^^^^^^^^^^^^^ `main` function is not allowed to be `async`
```

The why under E0752: an `async fn` returns a `Future<Output = T>`, a value that represents pending work. The work doesn't happen until something **polls** the future. Rust's standard library deliberately ships no built-in async runtime. So `async fn main()` would return a `Future` with nothing to drive it, and the compiler refuses to allow it.

This is *not* a "future would be silently dropped" runtime issue. It's a hard compile-time syntactic rule. The compiler refuses before any execution happens.

</details>

Run it:

```bash
cargo build -p lazydap-daemon
```

Read the error carefully. It's pointing at the right place. The compiler is refusing because Rust's standard library has no async runtime, and `async fn main` would return a future with nothing to poll it. You need a runtime to drive the future. That's what `tokio` is for.

---

## Step 2 — Add tokio (and the macro)

Add `tokio` to the workspace's dependencies. In the root `Cargo.toml`:

```toml
[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
```

`features = ["full"]` enables every tokio feature (the multi-threaded runtime, file I/O, networking, sync primitives, time, process spawning, signals). It's overkill at this stage. You only need the `macros` and `rt` features for `#[tokio::main]` to work. Lazydap takes the full set up front; you can revisit later if compile times bite.

Inherit it in `crates/daemon/Cargo.toml`:

```toml
[dependencies]
tokio = { workspace = true }
```

This is the same `*.workspace = true` inheritance pattern from chapter 01, applied to a dependency table instead of the package metadata table. The shape `tokio = { workspace = true }` is the canonical inheritance form for dependencies. (You'll see it written as `tokio.workspace = true` in some codebases. That also works.)

Now add the macro to `main.rs`:

```rust
#[tokio::main]
async fn main() {
    println!("hello from lazydap-daemon");
}
```

> 🔮 **Predict:** What does the macro actually generate? Try to write down the rough shape of the post-macro source.

<details>
<summary>Click after you've predicted</summary>

The expansion is approximately:

```rust
fn main() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        println!("hello from lazydap-daemon");
    })
}
```

The macro:

1. **Removed the `async` keyword from `fn main`.** `main` is now a normal sync function. That's what makes the compiler accept it.
2. **Built a tokio runtime** inside the new sync `main`.
3. **Wrapped the original body in an `async {}` block** and passed it to `runtime.block_on()`. `block_on` is the synchronous bridge: it runs the runtime and polls the future to completion before returning.

The async-ness is real but it lives *inside* `block_on`. The function called `main` is now sync. The compiler is happy.

This is what makes `#[tokio::main]` ergonomic. You write `async fn main` and get an async-feeling top-level, but it compiles to a sync entry point because that's what the OS expects.

</details>

Run:

```bash
cargo run -p lazydap-daemon
```

Expected output (after a longer first build because tokio is large):

```
   Compiling tokio v1.x.x
   Compiling lazydap-daemon v0.1.0 (.../lazydap/crates/daemon)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.21s
     Running `target/debug/lazydap-daemon`
hello from lazydap-daemon
```

Notice the layered story: chapter 01's workspace inheritance is what made `version.workspace = true` work in the daemon's Cargo.toml. Chapter 02 is what made the binary print something. The two compose.

---

## Step 3 — Two flavours of macro: attribute vs derive

`#[tokio::main]` is an **attribute macro**. The next thing you'll add is a **derive macro**. They're both procedural macros (compile-time source rewriters), but they have different powers and constraints.

| Flavour | Example | Where it can be applied | What it can do |
|---|---|---|---|
| Attribute macro | `#[tokio::main]` | Any item: function, struct, enum, module, impl block | Reshape the item arbitrarily. Rewrite the body. Replace the whole thing. |
| Derive macro | `#[derive(Parser)]` | Only structs and enums | *Only* generate `impl` blocks (and a few other items). Cannot modify the original struct/enum. |

The constraint on derive macros is what makes them safer. When you write `#[derive(Debug)]`, you can be confident the macro hasn't quietly rewritten your struct's fields. It can only *add*, typically an `impl Debug for YourStruct` block alongside the original definition.

Attribute macros are more powerful but also a bigger trust ask. `#[tokio::main]` literally rewrites your function. You can't see that without `cargo expand`.

---

## Step 4 — clap by derive

Add `clap` to the workspace dependencies:

```toml
[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
```

The `derive` feature flag is what enables `#[derive(Parser)]`. Without it, you'd have to use clap's older builder API.

Inherit in the daemon:

```toml
[dependencies]
tokio = { workspace = true }
clap = { workspace = true }
```

Now expand `main.rs`:

```rust
use clap::Parser;

/// lazydap daemon (placeholder — real one lands in M5).
#[derive(Parser, Debug)]
#[command(name = "lazydap-daemon", version, about)]
struct Args {
    /// Greeting message to print.
    #[arg(long, default_value = "hello from lazydap-daemon")]
    message: String,

    /// Number of times to repeat the greeting.
    #[arg(long, default_value_t = 1)]
    count: u32,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    for _ in 0..args.count {
        println!("{}", args.message);
    }
}
```

There are *three* macros at work here:

- `#[derive(Parser, Debug)]` adds `impl Parser for Args` (which gives you `Args::parse()`) and `impl Debug for Args`.
- `#[command(...)]` is a clap-specific helper attribute. Configures the parser. Only meaningful inside a struct that derives `Parser`.
- `#[arg(...)]` is also clap-specific. Configures one field's CLI surface.

The struct `Args` is just data. The `Parser` impl, generated by the derive macro, knows how to read `std::env::args()`, parse them according to the `#[command]` and `#[arg]` configuration, and either return an `Args` value or print usage and exit.

> 🔮 **Predict:** Run `cargo run -p lazydap-daemon -- --help`. How many distinct sources of information will end up in the help output? Count them.

<details>
<summary>Click after you've predicted</summary>

A common count is two: the field names, plus the doc comments on the fields. The doc comments and field names *are* sources, but there are more.

Run it:

```
$ cargo run -p lazydap-daemon -- --help
lazydap daemon (placeholder — real one lands in M5)

Usage: lazydap-daemon [OPTIONS]

Options:
      --message <MESSAGE>  Greeting message to print [default: "hello from lazydap-daemon"]
      --count <COUNT>      Number of times to repeat the greeting [default: 1]
  -h, --help               Print help
  -V, --version            Print version
```

Eight sources are at work here:

1. The doc comment **above the struct** ("lazydap daemon (placeholder — real one lands in M5)") is used because `#[command(... about)]` was set. clap pulls the description from the struct's doc comment.
2. `#[command(name = "lazydap-daemon")]` is the program name in `Usage:`.
3. The **field names** (`message`, `count`) become `--message` and `--count` flags.
4. The **field types** (`String`, `u32`) appear as `<MESSAGE>` and `<COUNT>` and constrain what's valid input. Try `--count abc` and watch clap reject it.
5. The **per-field doc comments** ("Greeting message to print", etc.) each become the human description after the flag.
6. `#[arg(default_value = "hello from lazydap-daemon")]` and `#[arg(default_value_t = 1)]` produce the `[default: ...]` markers.
7. `-h, --help` is auto-added by clap unconditionally.
8. `-V, --version` is auto-added because `version` was listed in `#[command(...)]`.

A single derive macro wires up eight different declarative inputs into one help output. That's the leverage you get for the price of a few annotations.

</details>

Now run with arguments:

```bash
cargo run -p lazydap-daemon -- --message hi --count 3
```

Output:

```
hi
hi
hi
```

The `--` separates cargo's own arguments from arguments passed to your binary. Without `--`, cargo would try to interpret `--message` as a cargo flag.

> 🔮 **Predict:** Run `cargo run -p lazydap-daemon -- --version`. What does it print, and where did the version number come from?

<details>
<summary>Click after you've predicted</summary>

```
$ cargo run -p lazydap-daemon -- --version
lazydap-daemon 0.1.0
```

The `0.1.0` flowed from `[workspace.package].version` (root `Cargo.toml`, set in chapter 01) → `version.workspace = true` (daemon's `Cargo.toml`) → cargo's `CARGO_PKG_VERSION` environment variable at compile time → clap's derive macro reading it → printed at runtime.

Chapter 01's workspace inheritance reaches all the way into runtime output of chapter 02's binary. The chapters compose. This is the ladder being visible.

</details>

---

## Try it yourself

> 🛠️ **Your turn:** Add a third argument: `--shout` (a boolean flag). When set, print the message in uppercase. Hint: clap converts `bool` fields automatically; the field type is `bool` and you don't need `default_value`.

After you've written it, run:

```bash
cargo run -p lazydap-daemon -- --message hello --count 2 --shout
```

Expected output:

```
HELLO
HELLO
```

If you got something different, common causes:

- "unexpected argument '--shout' found": you added the field but didn't decorate it with `#[arg(long)]`. Without `long`, clap doesn't know to expose it as a flag. The `bool` type is enough to make it a flag (no value needed) once `long` is set.
- The output is lowercase: you read the field but forgot to `.to_uppercase()` the string.
- `error[E0277]: the trait bound bool: clap::Args is not satisfied`: typo on the `#[arg(long)]` line, or you derived the wrong macro on the field.

Solution shape (don't peek until you've tried):

<details>
<summary>Click for the solution</summary>

```rust
#[derive(Parser, Debug)]
#[command(name = "lazydap-daemon", version, about)]
struct Args {
    /// Greeting message to print.
    #[arg(long, default_value = "hello from lazydap-daemon")]
    message: String,

    /// Number of times to repeat the greeting.
    #[arg(long, default_value_t = 1)]
    count: u32,

    /// Print the greeting in uppercase.
    #[arg(long)]
    shout: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let message = if args.shout {
        args.message.to_uppercase()
    } else {
        args.message
    };
    for _ in 0..args.count {
        println!("{message}");
    }
}
```

</details>

---

## Compiler conversation

Try this. In `main.rs`, change:

```rust
async fn main() {
```

to:

```rust
fn main() {
```

(Remove `async`, but keep `#[tokio::main]` above the function.)

Run `cargo build -p lazydap-daemon` and read the error. It will roughly say: "the function passed to `#[tokio::main]` must be async." The macro inspects the syntax of the function it's decorating and rejects any function that isn't `async`. This is concrete proof that the macro sees the *syntax* (including the `async` keyword) at compile time. A runtime decorator wouldn't have access to that information without reflection.

Restore the `async` keyword. Save.

If you have `cargo expand` installed (`cargo install cargo-expand`), now's a fun moment to peek:

```bash
cargo expand -p lazydap-daemon --bin lazydap-daemon
```

You'll see the post-macro source: a sync `fn main` that builds a tokio runtime, calls `block_on`, and runs the original body inside an async block. Don't get bogged down reading every line. Just confirm the *shape* matches the expansion you predicted earlier.

---

## What you can run now

```bash
cargo run -p lazydap-daemon -- --message hi --count 3
```

Output:

```
hi
hi
hi
```

```bash
cargo run -p lazydap-daemon -- --help
```

Output:

```
lazydap daemon (placeholder — real one lands in M5)

Usage: lazydap-daemon [OPTIONS]

Options:
      --message <MESSAGE>  Greeting message to print [default: "hello from lazydap-daemon"]
      --count <COUNT>      Number of times to repeat the greeting [default: 1]
  -h, --help               Print help
  -V, --version            Print version
```

**Ladder check.** Chapter 01 left you with `cargo build --workspace` succeeding and an empty library that did nothing. Now you have a *runnable* binary in a second crate that takes arguments and prints. The workspace inheritance from chapter 01 reaches into the daemon's `Cargo.toml` and into the version that clap prints at runtime. Chapter 02's binary is built on top of chapter 01's workspace, not in parallel to it.

Forward look: chapter 03 adds the conventions files (`rustfmt.toml`, `clippy.toml`, `rust-toolchain.toml`), the licenses, the `.gitignore`, and the GitHub Actions CI workflow that runs all of this automatically on push. Chapter 03 is light conceptually but heavy procedurally. It's the moment the project becomes a Project rather than a directory of files.

---

## Teach-back

Before moving on, answer these in your own words.

> 📣 **Q1:** Explain to a colleague who knows TypeScript: what's the difference between a TypeScript decorator and a Rust attribute macro? Use `#[tokio::main]` as the example.

> 📣 **Q2:** Why is `async fn main()` illegal in plain Rust? Why does adding `#[tokio::main]` make it legal?

> 📣 **Q3:** Attribute macros vs derive macros: what's each allowed to do, and which is more constrained?

> 📣 **Q4:** When you ran `cargo run -p lazydap-daemon -- --version` and it printed `0.1.0`, trace the path that `0.1.0` took from where it was originally written down to where it printed.

---

## Pain anchors covered

| New construct | The pain it solves | In which language |
|---|---|---|
| `#[tokio::main]` | Setting up an async runtime by hand: building a `Runtime`, calling `.block_on(...)` on the entry future, threading errors. The macro condenses ~5 lines of boilerplate into one annotation. | Rust ergonomics |
| `#[derive(Parser)]` from clap | Hand-rolling argument parsing in C with `getopt` / `argp`: error-prone, no help generation, no type validation, no defaults. clap's derive: write a struct, get the parser. | C, also Python's `argparse` boilerplate |
| Compile-time macros generally | TS decorators run at runtime, so they can't see source-level details (without reflection), can't generate types, and add runtime overhead. Rust macros run at compile time, see syntax, generate code, zero runtime cost. | TypeScript / Python |
| `#[command]`, `#[arg]` declarative attributes | Maintaining a parallel "config file" for CLI shape vs the data structure that consumes it. Derive macros let one struct definition be both. | Any language with separate CLI config |

---

## See also

- ← [Chapter 01: Cargo workspaces](01-cargo-workspaces.md)
- → [Chapter 03: Convention as code](03-conventions-as-code.md)
- [Underlying milestone: workspace setup](../implementation/00-workspace-setup.md)
- [tokio book: the runtime](https://tokio.rs/tokio/topics/bridging)
- [clap book: derive tutorial](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html)
- [The Little Book of Rust Macros](https://veykril.github.io/tlborm/) for when you're ready to write your own
- Anchor codebase: `mxr/crates/cli/src/main.rs` for the same pattern at production scale
