---
chapter: 4
session_id: M0-1
title: Hello, adapter
phase: A
estimated_time_minutes: 75
artifact: A binary that spawns codelldb (a real debug adapter) and prints its first chunk of output
prerequisites:
  - Chapter 03 (conventions-as-code) — workspace is conventions-complete
  - codelldb installed somewhere on PATH (we install in-chapter if not)
new_concepts:
  - Spawning external processes asynchronously with tokio::process::Command
  - Stdio piping vs inherit, the std-vs-tokio API split
  - kill_on_drop and the Drop trait (gloss only — deeper later)
  - Option::take and how mutability propagates through field access
  - The "trait must be in scope" gotcha (AsyncReadExt)
related_milestone: docs/implementation/tasks/M00-hello-adapter.md
---

# Chapter 04 — Hello, adapter

> Session ID: `M0-1` · Phase A · ~75 min · [Underlying milestone](../implementation/tasks/M00-hello-adapter.md)

## What you'll learn

How to spawn an external process asynchronously in Rust using **tokio**, capture its output stream, and clean it up safely. The vehicle is **codelldb**, the real LLDB-based debug adapter the rest of Phase A drives. By the end your code talks to a real debugger backend for the first time.

You'll also bump into three Rust-specific concepts that have no JavaScript or Python analog, with the compiler walking you through them: **the Drop trait** (gloss only), **mutability that propagates through field access**, and **the trait-must-be-in-scope rule**.

## What you'll build

A small example binary that spawns `codelldb`, reads the first chunk of bytes from its stderr, prints them, and exits. ~30 lines of code.

> By the end of this chapter, running:
> ```bash
> cargo run --example m0_hello_adapter
> ```
> will print something like:
> ```
> first stderr chunk: "[INFO  codelldb] Loaded \"/Users/.../liblldb.dylib\", version=\"lldb version 20.1.4-codelldb\"\n"
> ```
> That's something you couldn't do at the end of Chapter 03. Your program now talks to a real debugger.

## Before you start

**Prior knowledge assumed:**
- You know what spawning a child process means in *some* language (Node's `node:child_process`, Python's `subprocess.Popen`, Go's `os/exec`, C's `fork`/`execve` pair). Any one is enough.
- You're comfortable with async/await syntax. You already saw `#[tokio::main]` in Chapter 02.
- You've used pipes mentally (`ls | grep` in a shell). The chapter formalises the concept.

**Setup state required:**
- Workspace from Chapter 01: `cargo metadata --format-version 1` succeeds at the repo root.
- Daemon crate from Chapter 02: `cargo run -p lazydap-daemon -- --message hi --count 1` prints something.
- Conventions from Chapter 03: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets` exit clean.

If any of those fail, go back to the chapter that introduced the broken thing.

**One thing the chapter installs:** **codelldb**. Don't pre-install it. The install has a surprise the first time you do it, and you should hit that surprise here rather than later.

---

## Surface your model first

> 🤔 **Q1:** In Node/TypeScript, how would you spawn a child process, capture its stderr stream, read the first chunk of bytes, and then kill the process? Exact API names don't matter. The *shape* does: sync vs async, callbacks vs promises, what handles the streams expose.

> 🤔 **Q2:** In C, the same task uses four system calls in a careful dance. Do you know what they are and how they interact? Saying no is fine. The chapter fills it in.

<details>
<summary>Click after you've answered both</summary>

**Node shape (the calibration):**

```javascript
const { spawn } = require('node:child_process');
const child = spawn('codelldb', ['--port', '0'], { stdio: 'pipe' });
//        ^^ synchronous: returns immediately, child process is now running

child.stderr.on('data', (chunk) => console.log('stderr:', chunk.toString()));
//          ^^^^^^^^^^^^^^ async via callback — Readable stream emits 'data' events

child.on('exit', (code) => { /* cleanup */ });
//      ^^^^^^^^ another event you listen to
```

The clarification: **spawn is synchronous; the streams are async (event-based).** It's an easy thing to conflate. The spawn returns a `ChildProcess` immediately. Async only shows up when you read from stdout/stderr.

**C shape (the deeper one):**

Spawning a child and capturing its stderr in C is *four system calls in a careful dance*:

1. **`pipe`** — creates a pair of file descriptors `(read_fd, write_fd)`. Bytes written to `write_fd` come out of `read_fd`. This is your communication channel.
2. **`fork`** — clones the *entire current process*. Memory, file descriptors, everything. Now there are two of you. The child gets `pid==0`, the parent gets the child's actual pid.
3. **`execve`** — in the child only, replace the cloned process image with the new program (codelldb). Before doing this, you `dup2` the pipe's write_fd onto fd 2 (stderr) so the child's stderr writes go into your pipe.
4. **`waitpid`** — in the parent, block until the child exits. If you skip this, the child becomes a **zombie** (exited but uncollected, leaking until the parent dies).

Forget any step and you get orphans, zombies, fd leaks, undefined behaviour.

**Why this matters for what's next:** Both Node and tokio hide this dance behind `spawn`. Rust adds something Node doesn't: **automatic cleanup if your program panics or exits early**, via the Drop trait. You'll meet it shortly.

</details>

---

## Set up the example

The example file goes under `crates/daemon/examples/` because Cargo auto-discovers examples there as build targets of the `lazydap-daemon` crate. No `[[example]]` block in `Cargo.toml` needed. Convention over config, same idea as Next.js `app/` directory routing.

Create `crates/daemon/examples/m0_hello_adapter.rs` (empty file).

You also need `anyhow` as a dependency. Add to root `Cargo.toml`:

```toml
[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
anyhow = "1"
```

And to `crates/daemon/Cargo.toml`:

```toml
[dependencies]
tokio = { workspace = true }
clap = { workspace = true }
anyhow = { workspace = true }
```

`anyhow` gives you `anyhow::Result<T>`. "I just want errors to bubble up cleanly in this binary." Any error type that implements `std::error::Error` auto-converts via `?` into `anyhow::Error`. The split:

- **`anyhow`** → app code, examples, binaries. "Just give me one error type, I'll print it."
- **`thiserror`** → library code, typed APIs. "Callers want to match on specific error variants."

This chapter uses `anyhow`. You'll meet `thiserror` when lazydap's library error types land in Chapter 07.

> **Pain anchor:** in C, errors are `return -1; check errno; hope nothing reset it`. Rust's `Result<T, E>` makes errors part of the type signature, and `?` makes propagation a single character. You can't *forget* to handle an error. The compiler refuses.

---

## Concept 1 — `tokio::process::Command` vs `std::process::Command`

> 🔮 **Predict:** Three things show up at the top of the example:
> 1. Something to build and spawn a child process **asynchronously**
> 2. Something to enable `.read(&mut buf).await` on a child's stderr stream
> 3. Something to configure the child's stdio (piped vs inherited vs null)
>
> Which crate does each come from, `std` or `tokio`?

<details>
<summary>Click after you've predicted all three</summary>

| Item | Crate | Why |
|---|---|---|
| 1. Spawn async | **tokio** | std's Command is *blocking*; tokio mirrors std's API but every I/O method returns a future |
| 2. Read + await | **tokio** | Specifically `tokio::io::AsyncReadExt`, with a gotcha you'll hit |
| 3. Stdio config | **std** | `Stdio::Piped` is just a config enum; tokio reuses it |

**The pattern:** if a type *does* I/O, look in `tokio::*`. If it just *configures* something, look in `std::*`. You'll see this everywhere:

- `std::fs::File` → `tokio::fs::File`
- `std::net::TcpStream` → `tokio::net::TcpStream`
- `std::sync::Mutex` → `tokio::sync::Mutex` (with caveats)
- `std::process::Stdio` → reused by tokio

> **Why this split exists (the deeper why):** in Node, this distinction doesn't exist. Node is single-threaded async by default, so the spawn API is the one and only. In Rust, `std` is sync-by-default (the right call: std shouldn't depend on a runtime), and async runtimes layer themselves on top. Rust prioritises *no forced runtime*; the cost is two flavours of every I/O type.

</details>

Add the imports:

```rust
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
```

Rust convention groups imports by source: `std` first, then external crates, alphabetical within each group. `rustfmt` enforces this.

Add the `main` skeleton:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // we'll fill this in
    Ok(())
}
```

> 🔮 **Predict:** Why does `main` return `anyhow::Result<()>` instead of nothing? What does the `?` operator need to be able to do its job?

<details>
<summary>Click after you've predicted</summary>

The `?` operator is sugar for "if this is `Err`, early-return that `Err` from the enclosing function." For `?` to work, the **enclosing function must itself return a `Result`**. If `main` returned `()`, you couldn't `?` anywhere inside.

You'll use `?` on at least four calls in this chapter: `.spawn()?`, `.read(&mut buf).await?`, `std::str::from_utf8(...)?`, `child.kill().await?`. Each returns a different error type (`io::Error`, `Utf8Error`, ...). **Anyhow's job:** be the *single* return type that absorbs all of them via blanket trait impls.

`Ok(())` at the end means "made it through without erroring." `()` is Rust's **unit type**, a single zero-sized value meaning "no meaningful payload." Closest analogs: TS `void`, Python `None`. Not "an empty struct or array". It's its own thing.

</details>

Run `cargo check --workspace --all-targets`. The `--all-targets` flag is critical. Without it, examples don't get checked.

> **Pocket this:** Cargo's "default targets" = lib + bins. Examples, tests, and benches need `--all-targets` (or `--examples`, `--tests`, `--benches` individually). When `cargo build` says "fine" but CI breaks on examples, this is why. Your project's CI workflow already uses `--all-targets` for exactly this reason.

You should get a clean check (or unused-import warnings for the tokio imports, which the next steps resolve by actually using them).

---

## Concept 2 — The builder pattern for `Command`

You're about to write a chained sequence of calls that configure the spawn. Like this:

```rust
let mut child = Command::new("codelldb")
    .arg("--port")
    .arg("0")
    .env("RUST_LOG", "debug")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .spawn()?;
```

> 🔮 **Predict:** Why can you chain seven `.foo()` calls back-to-back like that? What pattern is this, and where have you seen it in JS/TS?

<details>
<summary>Click after you've predicted</summary>

This is the **builder pattern**. You've used it everywhere:

```javascript
$('div').addClass('foo').css('color', 'red').show()       // jQuery
knex('users').where('id', 1).select('name')               // Knex
[1,2,3].map(x => x*2).filter(x => x > 2).reduce(...)      // Array methods
```

Each call returns the same object (or a reference to it), so you chain. In Rust's `Command`:

- `Command::new("codelldb")` returns a **`Command`** value, a builder you can configure
- Each `.arg()`, `.env()`, `.stdout()`, `.stderr()`, `.kill_on_drop()` takes `&mut self`, mutates, and returns `&mut Self` so you keep chaining
- `.spawn()` is the **terminal** call that consumes the builder and *actually* launches the OS process. It returns `io::Result<Child>`, and `?` unwraps that into a `Child` (or early-returns on error).

**Critical:** `Command::new("codelldb")` does **not yet spawn anything**. It's just a config object. The OS process appears at `.spawn()`. So the line you're about to write does:

1. Build a config
2. Configure it (six chained methods)
3. **Then** spawn

Misunderstanding this is the most common Command bug. People think changes after `.spawn()` affect the running process. They don't.

</details>

---

## Concept 3 — `let mut child` and what `mut` actually does

In Rust, bindings are **immutable by default**. `let x = 5;` means you can't reassign `x` *and* you can't call mutating methods on it. To opt in to mutation: `let mut x`.

JS analog: `const` vs `let`. But Rust's `mut` is *deeper*. It doesn't just allow reassignment; it gates whether you can call methods that take `&mut self` on the value. (In JS, `const obj = {}; obj.foo = 1;` is fine. `const` only prevents *reassignment*, not *mutation through the binding*.)

You'll need `mut` here because later you'll call `child.kill().await?`, and `kill()` is `&mut self` on `Child`. Without `mut`, the compiler refuses.

There's a subtler reason coming, involving `Option::take()`. You'll hit it the experimental way.

---

## Concept 4 — `.kill_on_drop(true)` and the Drop trait

This is the conceptual gem of the chapter. One line pulls in one foundational Rust idea.

**Drop** is a Rust trait. When any value goes out of scope, its `Drop::drop()` method runs automatically. At compile time, the compiler *inserts* the cleanup call. You don't write a free, you don't write `try/finally`, you don't register an exit handler. The compiler does it.

For `Child` specifically:
- **Default behaviour:** Drop releases the *handle* but does **not** kill the OS process. The process keeps running. **Orphan.**
- **With `kill_on_drop(true)`:** Drop sends SIGKILL to the child first, then releases the handle. Process dies cleanly.

Why this matters: if `read` fails partway through, `?` propagates an error and `main` returns early. With `kill_on_drop(true)`, the still-running codelldb gets killed automatically because `child` goes out of scope as the stack unwinds. Same if `main` panics. Same if anything goes sideways.

> **Pain anchor (this is the big one):**
>
> - **Node:** if your script crashes mid-way, child processes orphan. People wire up `SIGINT` handlers, `process.on('exit', ...)`, or pull in libs like `tree-kill`. All ad-hoc, none bulletproof. You **will** ship this bug at some point.
> - **C:** worse. `fork` without `waitpid` → zombies. Crash before `waitpid` → orphan.
> - **Rust:** the OS process gets cleaned up because the language guarantees Drop runs on stack unwind, and `kill_on_drop(true)` makes that Drop kill the child. **Impossible to forget.** You opted in to the cleanup at construction time; the compiler enforces it from there.
>
> This is what people mean by *"ownership pays rent."* The borrow-checker fights you'll have later? This is part of what you're getting in exchange: automatic, exception-safe, can't-forget cleanup.

`Drop` shows up again in later chapters when you implement it for your own types. For today, this is enough: **value goes out of scope → Drop runs → child gets killed.**

---

## Write the spawn block

Replace the comment in your `main` body with:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // codelldb listens on TCP — with --port 0 it picks a free port and
    // logs "Listening on port N" to stderr (gated behind RUST_LOG=debug
    // in modern versions; see chapter notes).
    let mut child = Command::new("codelldb")
        .arg("--port")
        .arg("0")
        .env("RUST_LOG", "debug")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    // we'll read child.stderr next

    Ok(())
}
```

Why `RUST_LOG=debug` in there: codelldb is silent by default in newer versions. The "Listening on port N" message is gated behind the `RUST_LOG` environment variable, set to `debug` (or higher). Without it, the program would silently open a TCP listener and you'd see no output. The *why* shows up in a moment.

> **Useful gotcha (`Command::env` semantics):**
> Tokio's `.env(key, value)` **adds** to the inherited environment (so existing PATH, HOME, etc. flow through). Node's spawn with `{ env }` **replaces** the entire env unless you spread `process.env`. The Rust default is the friendlier one. To clear and replace in Rust: `.env_clear().env("RUST_LOG", "debug")`.

> 🔮 **Predict:** Run `cargo check --workspace --all-targets`. Should it pass clean, or do you expect a warning?

<details>
<summary>Click after you've predicted</summary>

You'll get warnings:

- `unused variable: child` — declared but not used yet
- `variable does not need to be mutable` — declared `mut` but no `&mut self` method called on it yet
- `unused import: tokio::io::AsyncReadExt` — imported but no AsyncReadExt method called yet

All three resolve once you add the read block next. **Don't** silence them by prefixing `_child`. The variable is about to get used.

(Quick gotcha to bank for later: `let _child = ...` is fine. It keeps Drop running normally. But `let _ = ...` (bare underscore, no name) has very different semantics: it drops the value **immediately**, which would kill codelldb before you read anything. Underscore-prefixed-name vs bare-underscore is a Rust footgun.)

</details>

---

## Concept 5 — `Option<ChildStderr>` and `.take()`

When `child` was returned from `.spawn()`, it had three optional stream handles:

- `child.stdout: Option<ChildStdout>`
- `child.stderr: Option<ChildStderr>`
- `child.stdin: Option<ChildStdin>`

**Why `Option`?** Because whether each stream exists depends on how you configured `Stdio`. With `Stdio::piped()`, the pipe exists → `Some(stream)`. With `Stdio::inherit()`, the child's stderr just goes to your terminal → `None`. The type system encodes "might not be there" at compile time.

> **Pain anchor:** in JS, `child.stderr` is just there. Sometimes null, sometimes a stream, and you'd find out at runtime by calling `.on()` on undefined. In C, you don't even have a struct field; you have to remember which fd you wired to which pipe. Rust's `Option<T>` makes "may or may not exist" part of the type. **You can't use a `T` until you've handled the `None` case.** No null pointer dereferences. Ever.

The **`.take()` method** (defined on `Option<T>`):
- Returns whatever was inside (`T`), and replaces it with `None`
- It **moves ownership** out of the `Option`. Once taken, the field is `None` forever.

This is *ownership in action*: a stream can only be read by one consumer, so the language enforces "you can only take it once."

```rust
let mut stderr = child.stderr.take().expect("stderr is piped");
```

`.expect("msg")` is `.unwrap()` with a custom panic message. The reason to use it here: you *know* the stream is `Some` because you configured `Stdio::piped()`. If that ever changes, the message documents the invariant you believed.

> **Mini-gotcha:** the workspace's clippy lints have `unwrap_used = "warn"`. `.unwrap()` would warn; `.expect()` does not (it's a separate lint, `expect_used`, which isn't enabled by default). Convention in production Rust: `.expect("invariant we believe")` is acceptable when you can articulate *why* it cannot fail. `.unwrap()` is for prototypes.

---

## Concept 6 — Stack-allocated byte buffer

```rust
let mut buf = [0u8; 256];
```

- `[T; N]` is **fixed-size array** syntax. `N` is a *compile-time constant*. The 256 bytes live on the stack, no heap.
- `0u8` is the literal zero typed as `u8` (one byte, 0..=255). Initial value of every slot.
- 256 bytes is enough because you're peeking at the first chunk; codelldb's startup logs are well under that.

> **JS analog:** closest is `new Uint8Array(256)`, but JS heaps that. Rust gives you stack vs heap as an *intentional choice* per allocation. Heap-allocated dynamic version would be `Vec<u8>`. **Pain anchor:** in C you malloc 256 bytes and remember to free; in Rust the stack array is freed automatically when it goes out of scope (and `Vec` does its own bookkeeping internally).

---

## Concept 7 — `read(&mut buf)` returns `usize` — and the partial-read gotcha

```rust
let n = stderr.read(&mut buf).await?;
```

- `.read(&mut buf)` writes bytes into your buffer, returns `io::Result<usize>` where `usize` is **how many bytes were actually written**.
- **`n` may be anything from `0` to `buf.len()`**, typically less than the full buffer. Async I/O delivers what's currently available; you might need multiple reads to get a full message.
- `n == 0` would mean EOF: child closed its stderr.
- `&mut buf` passes a *mutable borrow* of the buffer. Read writes into it without taking ownership.

This is a *foundational* truth about stream-based I/O: **never assume "one read = one message."** The pipe buffers what the writer sent; your `read` returns whatever's currently sitting there. Sometimes it's a complete message, sometimes a partial one, sometimes multiple coalesced together. Today's chapter sees one of those outcomes; chapter 05 introduces proper framing to handle all of them.

> **Pain anchor:** in C, this is the classic "I called read once and assumed I got the whole message" → garbage-corrupted-state production bug. In Node, it surfaces as `'data'` events that don't align with logical messages. Rust doesn't make this go away. It makes the type signature *honest* (`read` returns `usize`, not "one message"). The compiler can't help you here; the protocol design has to.

---

## Concept 8 — The trait-must-be-in-scope gotcha

`.read(&mut buf).await` only compiles because of an import you wrote at the top:

```rust
use tokio::io::AsyncReadExt;
```

`.read()` is **not** an inherent method on `ChildStderr`. It's defined on a trait called `AsyncReadExt`. In Rust:

- An **inherent method** is defined on the type itself with `impl Type { ... }`.
- A **trait method** is defined on a trait, and only *callable when the trait is in scope*.

The pattern you'll see constantly:

- `AsyncRead` is the *core* trait. It defines the low-level "can be read from asynchronously" contract.
- `AsyncReadExt` is an *extension* trait with convenience methods (`.read()`, `.read_exact()`, `.read_to_end()`, etc.) auto-implemented for every type that implements `AsyncRead`.

If you forget the import, the compiler complains:

```
error[E0599]: no method named `read` found for struct `ChildStderr` in the current scope
help: items from traits can only be used if the trait is in scope
help: the following trait is implemented but not in scope: AsyncReadExt
```

Good compiler pedagogy: the message tells you *exactly* what's missing.

> **Pain anchor:** in C, "polymorphism" means rolling your own vtable (struct of function pointers): painful and easy to forget to wire up. Traits give you the same thing with compile-time enforcement: the compiler refuses to call `.read()` on something whose `read` impl isn't in scope. Forgetting is impossible.

---

## Write the read block

Add this between `let mut child = ...` and the closing `Ok(())`:

```rust
let mut stderr = child.stderr.take().expect("stderr is piped");
let mut buf = [0u8; 256];
let n = stderr.read(&mut buf).await?;
let s = std::str::from_utf8(&buf[..n])?;
println!("first stderr chunk: {s:?}");

child.kill().await?;
```

A few new things to notice:

- **`&buf[..n]`** is a *slice* of the first `n` bytes. Range syntax. Type is `&[u8]` (borrowed byte slice).
- **`std::str::from_utf8`** returns `Result<&str, Utf8Error>`. It validates UTF-8 along the way. `?` propagates if codelldb sent invalid UTF-8 (it won't, but the type forces you to acknowledge it could).
- **`{s:?}`** is a Rust format string with **Debug** formatting (the `:?`). For strings this shows quotes and escapes (`"Listening on port 53274\n"` instead of just the literal). Useful for inspection.
- **`child.kill().await?`** is the explicit, awaitable kill. Belt-and-suspenders with `kill_on_drop(true)`: the explicit kill is *deterministic and awaitable*; `kill_on_drop` is the *safety net* for `?`-early-exit paths. Both layers are cheap; together they make orphans impossible.

> **Pain anchor:** in C, "is this `char*` valid UTF-8" is a question with no answer at the type level. You just hope. Rust's `str::from_utf8` is the only way to go from `&[u8]` to `&str`, and it returns a `Result`. Garbage strings get caught at the type-system boundary.

---

## Compiler conversation: experiment with `mut`

Time to prove a claim from earlier the Rust way: let the compiler tell you. Temporarily change:

```rust
let mut child = Command::new("codelldb")
```

to:

```rust
let child = Command::new("codelldb")
```

Run `cargo check --workspace --all-targets`.

> 🔮 **Predict before reading the error:** what line will the compiler complain about, and why?

<details>
<summary>Click after you've predicted</summary>

You'll get something like:

```
error[E0596]: cannot borrow `child.stderr` as mutable, as `child` is not declared as mutable
  --> crates/daemon/examples/m0_hello_adapter.rs:17:22
   |
17 |     let mut stderr = child.stderr.take().expect("stderr is piped");
   |                      ^^^^^^^^^^^^ cannot borrow as mutable
   |
help: consider changing this to be mutable
   |
 9 |     let mut child = Command::new("codelldb")
   |         +++
```

The teaching moment: **`Option::take()` is `&mut self`**. Its signature is:

```rust
pub fn take(&mut self) -> Option<T>
```

It mutates the `Option<T>` in place to leave `None` behind. So calling `.take()` on `child.stderr`:
1. Needs `&mut child.stderr` (mutable borrow of the field)
2. Which needs `&mut child` (mutability propagates *up* through field access)
3. Which needs the binding declared as `let mut child`

**Conceptual gem to pocket:** `Option::take()` *looks* passive ("just extract the value") but mechanically is a **mutation**. It overwrites the Option in place. The type system makes this visible. In JS you'd write `const stream = child.stderr; child.stderr = null;` and call it manual; here `.take()` is the atomic version, and the compiler enforces that the holder must allow mutation.

**Bonus:** notice the compiler told you exactly where to fix it (`9 | let mut child = ... | +++`). Rust's diagnostic UX is genuinely pedagogical. Always read the `help:` lines. Now restore the `mut`.

</details>

Restore `let mut child = ...`. Run `cargo check --workspace --all-targets`. Should be clean (or with a single unused-mut warning you already accepted).

---

## Try it: install codelldb if you don't have it

If `which codelldb` fails for you, you need to install it. The project's [`CONTRIBUTING.md`](../../CONTRIBUTING.md) has the canonical install instructions; the short version (Apple Silicon):

```bash
curl -sL -o /tmp/codelldb.vsix \
  https://github.com/vadimcn/codelldb/releases/latest/download/codelldb-darwin-arm64.vsix
mkdir -p ~/.local/opt/codelldb
unzip -q -o /tmp/codelldb.vsix -d ~/.local/opt/codelldb
```

Then create a wrapper script (NOT a symlink, see below for why) at `~/.local/bin/codelldb`. Make it a one-line bash script that runs the real binary at the absolute path `~/.local/opt/codelldb/extension/adapter/codelldb`, passing through all arguments. Mark it executable.

```bash
codelldb --help    # confirms it runs
```

**Important:** use the **wrapper script**, not a symlink. codelldb computes its `liblldb.dylib` location relative to `argv[0]`. When invoked via a symlink, it computes the wrong path and panics. The wrapper script invokes the real binary with an absolute path, sidestepping the issue. See [`docs/reference/codelldb-quirks.md`](../reference/codelldb-quirks.md) for the full story.

---

## Try it: run the example

```bash
cargo run --example m0_hello_adapter
```

> 🔮 **Predict:** What text will print? Exact wording doesn't matter, just the *shape* of the output.

<details>
<summary>Click after you've predicted</summary>

You should see something like:

```
first stderr chunk: "[INFO  codelldb] Loaded \"/Users/.../liblldb.dylib\", version=\"lldb version 20.1.4-codelldb\"\n"
```

A single log line from codelldb, captured as bytes, decoded as UTF-8, printed back with Debug formatting (hence the escaped quotes and `\n`).

**Note what's *not* there:** the `Listening on port N` message. Why? Because codelldb's logger flushes each log call as a *separate* write to stderr. Your single `read` pulled in whatever was buffered when you called it; the next log line landed slightly later, and you'd already returned. Live demonstration of the partial-read gotcha from Concept 7.

This is fine. The chapter's promise was "first contact" and you've made it. Chapter 05 fixes the partial-read problem with proper Content-Length-based message framing.

</details>

If you got something different:
- **Hung silently for many seconds?** Your codelldb might be silent without `RUST_LOG=debug`. Confirm the `.env("RUST_LOG", "debug")` line is in your example.
- **Panic about `liblldb.dylib`?** Your codelldb install used a symlink instead of a wrapper script. See [`docs/reference/codelldb-quirks.md`](../reference/codelldb-quirks.md).
- **`codelldb: command not found`?** Install it (instructions above) and confirm `~/.local/bin` is on your `PATH`.

---

## What you can run now

```bash
cargo run --example m0_hello_adapter
```

You'll see the codelldb load message print, then the program exits cleanly with `child` dropped (and `kill_on_drop` ensuring codelldb died).

**Ladder check:**

- Chapter 01: you had a workspace.
- Chapter 02: you had a daemon binary that prints args.
- Chapter 03: you had a conventions-pinned, CI-ready repo.
- **Chapter 04: you have lazydap making external contact with a real debugger backend.**

Next chapter (Chapter 05): you'll loop reads, parse `Content-Length` headers, and handle the partial-read problem properly. After that, lazydap is reading framed DAP messages.

---

## Teach-back

Before moving on, answer in your own words. If you can't, re-read the relevant section.

> 📣 **Q1:** Imagine explaining to a junior Node engineer who has never touched Rust: "What does `kill_on_drop(true)` do, and what does it give you that Node has no equivalent for?" (~2 sentences.)

> 📣 **Q2:** "You wrote `let mut child` instead of `let child`. The mutability isn't because `child` gets reassigned. It's because of one specific line. Which line, and why?" (1 sentence.)

> 📣 **Q3:** "The example printed only one log line, missing the 'Listening on port N' line you expected. Was that a bug in your code? If not, what does it teach you about stream-based I/O, and what will Chapter 05 do about it?" (2 sentences.)

---

## Pain anchors covered

| New construct | The pain it solves | In which language |
|---|---|---|
| `tokio::process::Command` | Sync `std::Command` blocks the thread, freezing all other tasks on the runtime | Rust (vs JS where there's no thread to block) |
| `Stdio::piped()` | C's pipe + dup2 + fork ordering dance | C |
| `kill_on_drop(true)` | Orphaned child processes when the parent crashes mid-run | Node, C |
| `Option<T>` | NULL pointer dereference / "undefined is not a function" | C, JS, Java |
| `Option::take()` | The "two consumers tried to read the same stream" bug | All languages with shared mutable state |
| `let mut child` | C's "all variables can mutate". Rust makes mutation an opt-in declared at the binding site | C, JS |
| Drop trait (gloss) | malloc/free pairing burden, `try/finally`, manual `kill()` in error paths | C, Java, Node |
| `read(&mut buf) -> usize` | "I called read once and assumed I got the whole message" production bug | All stream protocols |
| `str::from_utf8` | "Is this `char*` valid UTF-8?" with answer at runtime, often a crash | C |
| Trait must be in scope | None (this is genuinely Rust-specific) | — |

---

## See also

- ← [Chapter 03: Convention as code](03-conventions-as-code.md)
- → Chapter 05: Read one message *(coming soon — partial-read problem solved with Content-Length framing)*
- [Underlying milestone: M0 — Hello, adapter](../implementation/tasks/M00-hello-adapter.md)
- [`docs/reference/codelldb-quirks.md`](../reference/codelldb-quirks.md) — the install footguns this chapter dodges
- [`CONTRIBUTING.md`](../../CONTRIBUTING.md) — DAP adapter setup
