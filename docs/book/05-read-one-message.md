---
chapter: 5
session_id: M1-1
title: Read one message
phase: A
estimated_time_minutes: 90
artifact: A binary that connects to codelldb over TCP, sends an initialize request, and pretty-prints the framed JSON response
prerequisites:
  - Chapter 04 (hello-adapter) — codelldb installed, can be spawned, stderr can be read
  - Cargo workspace + daemon crate from earlier chapters
new_concepts:
  - Async byte-stream parsing — read_line for terminator-known headers, read_exact for size-known body
  - BufReader for syscall amortisation
  - The lines() move-out footgun — lines(self) consumes the BufReader
  - Content-Length framing as inherited from HTTP via LSP
  - Version drift between docs and live tools
related_milestone: docs/implementation/tasks/M01-read-one-message.md
---

# Chapter 05 — Read one message

> Session ID: `M1-1` · Phase A · ~90 min · [Underlying milestone](../implementation/tasks/M01-read-one-message.md)

## What you'll learn

How to parse one framed message off a TCP byte stream — the smallest possible "framed protocol parser." Vehicle: codelldb's `initialize` response, which uses the same `Content-Length: N\r\n\r\n` framing as HTTP and LSP. By the end you've made first contact with the **Debug Adapter Protocol** itself, not just its transport.

Three Rust constructs land today: `read_line` (terminator-known reads), `read_exact` (size-known reads), and `BufReader` (the wrapper that makes both efficient). Plus one ownership lesson the compiler will teach you directly: the `lines()` method consumes its receiver, which surfaces a footgun that bites in this exact code path.

## What you'll build

A small example binary that spawns codelldb, parses its TCP port from stderr, connects, sends an `initialize` request, reads exactly one Content-Length-framed JSON response, pretty-prints it, and exits.

> By the end of this chapter, running:
>
> ```bash
> cargo run --example m1_read_one_message
> ```
>
> will print something like:
>
> ```
> codelldb listening on port 50417
> ---- DAP response ----
> {
>   "body": {
>     "exceptionBreakpointFilters": [...],
>     "supportsConfigurationDoneRequest": true,
>     "supportsConditionalBreakpoints": true,
>     ...
>   },
>   "command": "initialize",
>   "request_seq": 1,
>   "seq": 1,
>   "success": true,
>   "type": "response"
> }
> ```
>
> Real DAP. Real capability flags. Your code is now talking the actual protocol, not just spawning a process.

## Before you start

**Prior knowledge assumed:**
- You finished chapter 04 — codelldb is installed, the wrapper script works, `cargo run --example m0_hello_adapter` prints a stderr chunk.
- You're comfortable with what a TCP socket is conceptually: a byte stream between two endpoints, identified by `(IP, port)`.
- You've written at least one HTTP request handler in some language. The shape of "headers, blank line, body" should be familiar.

**Setup state required:**

- `cargo run --example m0_hello_adapter` exits cleanly and prints a codelldb log line.
- `which codelldb` resolves to the wrapper script from chapter 04.

If either of those fails, fix it before continuing.

---

## Surface your model first

> 🤔 **Q:** Forget Rust. In Node (or Python, or whatever you reach for first), suppose I gave you a TCP socket and told you "the other side sends framed messages — first there's a header `Content-Length: 247\r\n\r\n`, then exactly 247 bytes of JSON. Read one message." Describe the shape of the code:
>
> 1. How would you read the header — byte by byte, line by line, or read-everything-and-parse?
> 2. How would you stop reading the header and switch to reading the body?
> 3. How would you make sure you read exactly 247 bytes for the body — no more, no less?
> 4. What happens if you read 300 bytes by accident? Where do the extra 53 go?

<details>
<summary>Click after you've answered</summary>

A common first answer is buffer-oriented: "read everything as a string, split on `\r\n\r\n`, parse the header lines, count the bytes for the body." That mental model is correct *for a message that's already entirely in memory* — `await fetch(...).then(r => r.text())` lets you do this. Total bytes are known; you split at your leisure.

The break: in the streaming case, you don't have "the whole string." You have:

- a socket
- a `read(buf)` call that returns "whatever's available right now, between 1 and `buf.len()` bytes"
- no signal in those bytes that says "this is one message"

So if you say "let me read everything and split," you have to know **when to stop reading.** And you can't, because reading past the body gets you the *next* message, or hangs forever waiting for it. **The header is exactly what tells you how much body to read.** Chicken and egg in one direction.

So the actual shape of streaming code is two phases:

1. Read **just enough** to find `\r\n\r\n`. Not one byte more.
2. Parse the header lines you've collected. Pull out `Content-Length: 247`.
3. Now read **exactly 247 bytes**. Not 246. Not 248.

Two operations, two different tools. That's why this chapter introduces both `read_line` and `read_exact`.

</details>

---

## Side quest: HTTP servers solve the same problem

This isn't an analogy. It's the literal same scheme. DAP didn't invent its framing; it borrowed from LSP, which borrowed from HTTP/1.1.

**An HTTP/1.1 request:**

```
POST /api HTTP/1.1\r\n
Host: example.com\r\n
Content-Type: application/json\r\n
Content-Length: 247\r\n
\r\n
{"user":"alice","action":"login",...}
```

**A DAP message:**

```
Content-Length: 247\r\n
\r\n
{"seq":1,"type":"request","command":"initialize",...}
```

Same shape. Header lines terminated by `\r\n`, blank `\r\n` between headers and body, body of exactly `Content-Length` bytes.

The smallest substantive difference: HTTP has a request/status line as the very first line (`POST /api HTTP/1.1` or `HTTP/1.1 200 OK`); DAP starts with headers from byte 0. Otherwise the parser shape is identical — and every Express route handler you've ever written got `req.body` from someone running this exact `read_line`-headers-then-`read_exact`-body loop underneath.

> **Pocket this:** "framing on top of a byte stream" is one of the most-reused patterns in computing. HTTP, DAP, LSP, Redis (RESP), Memcached, IRC, SMTP, IMAP — all variants of the same trick. Different terminators or length schemes, same idea.

> **Pain anchor:** in C you implement this by hand with `recv()` calls and a manual buffer. Buffer-overrun bugs in hand-rolled framing parsers are a classic CVE category. Rust's `BufReader` gives you the same thing as a battle-tested type — same idea, no overrun risk.

---

## Concept 1 — Three tools, one rule

> 🔮 **Predict:** Two tools fit the two phases naturally. Match each:
>
> - Phase 1 (the headers): you don't know the total size in advance, but you know the terminator (`\r\n` for each line, then a blank `\r\n` for "headers done"). What kind of read primitive suits this?
> - Phase 2 (the body): you know the exact byte count (247) before you start. What kind of read primitive suits this?

<details>
<summary>Click after you've predicted</summary>

| Phase | Primitive | Why |
|---|---|---|
| 1 (headers) | `read_line` | Terminator known. Read until `\n`, return what we got. |
| 2 (body) | `read_exact` | Count known. Read until N bytes have arrived; never less. |

`read_exact` does *not* mean "one big read." Under the hood it loops over multiple smaller reads if the kernel hands back fewer bytes than asked. The "exact" is about the *return contract*: you handed it a buffer of size N, you get back N bytes (or an error). Partial reads are absorbed.

</details>

There's a third tool — **`BufReader`** — that wraps your raw stream and amortises the cost of all those small reads. Before introducing it, predict why naive byte-by-byte reading is bad:

> 🔮 **Predict:** Imagine implementing `read_line` over the **raw socket** without any buffering — call `read(&mut [0u8; 1])` repeatedly, one byte at a time, append to a string, stop on `\n`. Why is this a bad idea? What's it making the kernel do that it shouldn't have to?

<details>
<summary>Click after you've predicted</summary>

Each `read()` is a **syscall** — a transition from user space into the kernel and back. Mode switch, register save/restore, the scheduler may even reschedule you. Hundreds of nanoseconds to a few microseconds *per call*. For a 120-byte header (typical), that's 120 syscalls instead of one.

`BufReader` wraps the stream:

- Has its own internal buffer (~8 KiB by default)
- First call: one big `read()` syscall fills the internal buffer
- Subsequent `read_line` / `read_exact` calls scan within that buffer at memory speed
- When the buffer empties, refills with another single syscall

Mental picture: instead of running to the warehouse for each item, you take a truck once, fill it, then unload from the truck. The kernel boundary is the warehouse; BufReader is the truck.

</details>

> ⚠️ **Footgun to bank now** (it will bite later in this chapter): once you wrap a stream in a `BufReader`, the bytes already pulled into the BufReader's internal buffer are **only accessible through the BufReader**. If you read headers via `BufReader` and then try to read the body from the raw stream, you'll lose whatever bytes the BufReader already buffered past `\r\n\r\n`. **The same BufReader instance has to do both reads.**

Mark this. The compiler will enforce it on you in a moment.

---

## Concept 2 — TCP socket vs Unix socket (brief)

codelldb listens on TCP. Later in lazydap (M5), our daemon will use a **Unix domain socket** for IPC. Quick comparison:

| | TCP | Unix domain socket |
|---|---|---|
| **Address** | `(IP, port)` | A filesystem path |
| **Scope** | Cross-machine | Same machine only |
| **Cost** | TCP stack: IP headers, checksums, congestion control | Memcpy through the kernel |
| **Access control** | None built-in | Filesystem permissions |

From your program's view, the API is almost identical: connect, read, write, close. Both are byte-stream sockets. The framing problem we're tackling today is the same regardless of which transport sits underneath.

codelldb chose TCP for portability (works on every OS, can debug remotely if needed). lazydap will choose Unix sockets for IPC because it's local-only, faster, and gets file-permission auth for free. **The protocol on top is independent of the transport.** That's the whole point.

---

## Set up the example

Add `serde_json` to the workspace dependencies in the root `Cargo.toml`:

```toml
[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
anyhow = "1"
serde_json = "1"
```

And to the daemon's `crates/daemon/Cargo.toml`:

```toml
[dependencies]
tokio = { workspace = true }
clap = { workspace = true }
anyhow = { workspace = true }
serde_json = { workspace = true }
```

Create `crates/daemon/examples/m1_read_one_message.rs` (empty file).

---

## Phase 1+2 — Spawn codelldb, parse the port

The shape extends what you already know from chapter 04: spawn with `Command`, pipe stderr, drain in the background. The new step: parse the port out of a stderr line.

```rust
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::Command;

async fn spawn_codelldb_and_get_port() -> anyhow::Result<(tokio::process::Child, u16)> {
    let mut child = Command::new("codelldb")
        .arg("--port")
        .arg("0")
        .env("RUST_LOG", "debug")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stderr = child.stderr.take().expect("stderr is piped");
    let mut lines = BufReader::new(stderr).lines();

    while let Some(line) = lines.next_line().await? {
        let Some((_, rest)) = line.split_once("Listening on ") else {
            continue;
        };
        // Modern codelldb (20.x): "Listening on 127.0.0.1:NNNNN".
        // Older versions: "Listening on port NNNNN". Tolerate both.
        let port_str = rest
            .strip_prefix("port ")
            .unwrap_or_else(|| rest.rsplit(':').next().unwrap_or(rest));
        let port: u16 = port_str.trim().parse()?;
        // Drain the rest of stderr in the background.
        tokio::spawn(async move {
            while let Ok(Some(_)) = lines.next_line().await {}
        });
        return Ok((child, port));
    }
    anyhow::bail!("codelldb did not print a 'Listening on' line")
}
```

Two notable shapes:

- **`BufReader::new(stderr).lines()`** — wraps stderr, returns an async line iterator. `.next_line().await` returns `Ok(Some(line))`, `Ok(None)` for EOF, or `Err(e)`.
- **`tokio::spawn(async move { ... })`** — fire-and-forget the drain task. Without this, codelldb's stderr pipe fills up (~64 KiB kernel buffer) and the adapter blocks silently when it next tries to log. This pattern carries forward to every long-running subprocess you ever spawn.

> **Aside on version drift:** the matcher above tolerates two log formats because codelldb's logging changed between major versions. Older codelldb logged `"Listening on port NNNNN"`; current logs `"Listening on HOST:PORT"`. The defensive `strip_prefix("port ")` + `rsplit(':')` handles both. (See [`docs/issues/0002-codelldb-version-drift-rust-log.md`](../issues/0002-codelldb-version-drift-rust-log.md) for the full story.)

---

## Phase 3 — TCP-connect and send the request

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (mut child, port) = spawn_codelldb_and_get_port().await?;
    println!("codelldb listening on port {port}");

    let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;

    let request = serde_json::json!({
        "seq": 1,
        "type": "request",
        "command": "initialize",
        "arguments": {
            "clientID": "lazydap",
            "clientName": "lazydap",
            "adapterID": "lldb",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "pathFormat": "path",
        },
    });
    let request_bytes = serde_json::to_vec(&request)?;
    let header = format!("Content-Length: {}\r\n\r\n", request_bytes.len());

    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&request_bytes).await?;
    stream.flush().await?;

    // Phase 4 goes here.

    child.kill().await?;
    Ok(())
}
```

A few mechanical APIs:

| Code | What it does |
|---|---|
| `serde_json::json!({...})` | Macro that builds a `serde_json::Value` (untyped JSON) from JSON-like syntax |
| `serde_json::to_vec(&value)?` | Serialise `Value` → `Vec<u8>` of UTF-8 JSON bytes |
| `stream.write_all(bytes).await?` | Same idea as `read_exact` in the other direction: keeps writing until *all* bytes are out |
| `stream.flush().await?` | Tell the kernel to actually push buffered bytes out — defensive belt-and-suspenders |
| `TcpStream::connect((host, port))` | Async TCP connect; the tuple converts to a socket address automatically |

> 🔮 **Predict:** Why two `write_all` calls — one for the header, one for the body — instead of concatenating into a single buffer?

<details>
<summary>Click after you've predicted</summary>

No protocol reason. Either works. Two writes is just clearer to read — header then body matches the mental model. The TCP layer doesn't care; it batches bytes as it sees fit. If you wanted to optimise: build a `Vec<u8>` with header bytes followed by body bytes, single `write_all`, no `flush`. One fewer syscall.

</details>

---

## Phase 4 — The header loop and the body read

This is the conceptual moment. You'll write it. There are some hints in the comments below; the right way to engage is to type the loop yourself, hit a compile error or two, and let the compiler walk you through.

The shape:

```rust
// Wrap the stream in a BufReader. The same instance reads headers AND body.
let mut reader = BufReader::new(&mut stream);
let mut buf = String::new();
let mut content_length: Option<usize> = None;

// Header loop: read line by line until we hit an empty line.
// Pull Content-Length out as we go.
loop {
    buf.clear();
    let n = reader.read_line(&mut buf).await?;
    if n == 0 {
        anyhow::bail!("EOF before end of headers");
    }
    let line = buf.trim_end_matches(['\r', '\n']);
    if line.is_empty() {
        break;
    }
    if let Some(rest) = line.strip_prefix("Content-Length:") {
        content_length = Some(rest.trim().parse()?);
    }
}

let content_length =
    content_length.ok_or_else(|| anyhow::anyhow!("no Content-Length header"))?;

// Body: exactly content_length bytes, on the same BufReader.
let mut body = vec![0u8; content_length];
reader.read_exact(&mut body).await?;

// Pretty-print.
let value: serde_json::Value = serde_json::from_slice(&body)?;
println!("---- DAP response ----");
println!("{}", serde_json::to_string_pretty(&value)?);
```

A handful of details worth noting:

- **`Option<usize>` for `content_length`** — distinguishes "no header found" (an error) from "header was 0" (technically valid). Using a default of `0` would silently read 0 bytes, which is a worse failure mode.
- **`buf.clear()` at the top of each iteration** — `read_line` *appends* to the buffer. Forgetting to clear means line N+1 contains lines 1 through N+1 concatenated.
- **`if n == 0` is EOF** — `read_line` returns `Ok(0)` only when the writer closed. Any non-EOF read returns at least 1 byte (the trailing `\n` or the line content + `\n`).
- **`trim_end_matches(['\r', '\n'])`** — `read_line` includes the terminator in the buffer. Trim before checking content. The empty line check (`line.is_empty()`) becomes a clean break condition.
- **`reader.read_exact(...)`** for the body, on the same BufReader instance. Don't go back to the raw `stream`. The BufReader has likely already buffered some body bytes during the last `read_line` call; reading from `stream` would skip them.

---

## Compiler conversation: the `lines()` move-out footgun

This is the predicted footgun. If you tried to use the `lines()` iterator API for the header loop instead of `read_line`, you'd hit:

```rust
let mut reader = BufReader::new(&mut stream);
let mut lines = reader.lines();             // hmm
while let Some(line) = lines.next_line().await? {
    // ... process header ...
}
reader.read_exact(&mut body).await?;        // ERROR
```

Compiler:

```
error[E0382]: borrow of moved value: `reader`
   --> ...
    |
    | let mut reader = BufReader::new(&mut stream);
    |     ---------- move occurs because `reader` has type ...
    | let mut lines = reader.lines();
    |                        ------- `reader` moved due to this method call
    | reader.read_exact(&mut body).await?;
    | ^^^^^^ value borrowed here after move

note: `tokio::io::AsyncBufReadExt::lines` takes ownership of the receiver `self`, which moves `reader`
   --> async_buf_read_ext.rs:348:18
    |
348 |         fn lines(self) -> Lines<Self>
    |                  ^^^^
```

Read it carefully. The signature `fn lines(self) -> Lines<Self>` takes **`self`**, not `&mut self`. Calling `lines()` *moves* the BufReader into the returned `Lines` iterator. After that, the original `reader` variable is dead — you can't use it for `read_exact`.

This is exactly the BufReader rule from concept 1 ("same instance for both reads"), enforced by Rust's ownership system at compile time. The compiler refuses to let you make the mistake.

The fix: use `read_line(&mut buf)` directly. Its signature is `fn read_line(&mut self, ...) -> ...` — `&mut self` borrows, doesn't move. The BufReader stays alive after each call.

> **Bank this:** when an extension trait offers two methods that look superficially equivalent — one returning a "stream of T" iterator, one borrowing for a single op — check the signatures. The streaming variant typically takes `self` by value and consumes the receiver. If you need to keep the receiver, use the borrowing variant.

---

## Try it

```bash
cargo run --example m1_read_one_message
```

Expected output:

```
codelldb listening on port 50417
---- DAP response ----
{
  "body": {
    "exceptionBreakpointFilters": [
      {
        "default": true,
        "filter": "cpp_throw",
        "label": "C++: on throw",
        "supportsCondition": true
      },
      ...
    ],
    "supportTerminateDebuggee": true,
    "supportsCancelRequest": true,
    "supportsConditionalBreakpoints": true,
    "supportsConfigurationDoneRequest": true,
    ...
  },
  "command": "initialize",
  "request_seq": 1,
  "seq": 1,
  "success": true,
  "type": "response"
}
```

The exact list of `supports*` flags varies with codelldb version. The shape is the same: `"command": "initialize"`, `"success": true`, a `body` full of capability flags advertising what this adapter can do. **Real DAP. You're now reading the actual protocol.**

If you got something different:

- **Hung silently with no output past `codelldb listening on port N`?** Your matcher might be off; run `RUST_LOG=debug codelldb --port 0` directly and check the actual log line format.
- **Hung silently with no output at all?** The matcher in `spawn_codelldb_and_get_port` didn't match. Same fix as above — check the log format directly.
- **JSON parse error?** Body read consumed the wrong number of bytes — likely missing the empty-line check that breaks the header loop, or going back to the raw stream for the body read instead of the BufReader.
- **`codelldb: command not found`?** Chapter 04's setup needs to be in place first.

---

## What you can run now

```bash
cargo run --example m1_read_one_message
```

A real DAP `initialize` response, captured and pretty-printed. Real protocol, real adapter, real capability flags.

**Ladder check:**

- Chapter 04: you spawned codelldb and read its first stderr chunk (one `read`, partial-read problem visible).
- **Chapter 05: you're reading the actual debug adapter protocol.** One framed message decoded byte-perfect.

Next chapter (chapter 06) introduces typed serde — instead of `serde_json::Value` (untyped JSON), you'll define `DapRequest`, `DapResponse`, `Capabilities` as Rust types with derive macros, and let the compiler check the protocol shape for you.

---

## Teach-back

> 📣 **Q1:** "What does `BufReader` actually do — and what specifically about its `lines()` method made the compiler refuse to let you use the BufReader after calling it?" Pin both halves: the *what* (syscall amortisation, lazy fill) and the *consume-vs-borrow signature* (`fn lines(self)`).

> 📣 **Q2:** "After the headers, you can't read the body from the raw `TcpStream` — you have to read it through the same `BufReader`. Why? What's in the BufReader that isn't in the stream anymore?"

> 📣 **Q3:** "Frame this DAP message-reading code against an HTTP/1.1 request parser. What's the same? What's the smallest substantive difference?"

---

## Pain anchors covered

| New construct | The pain it solves | In which language |
|---|---|---|
| `BufReader` | Naive byte-by-byte reads issue one syscall per byte → 100x overhead. C programmers roll their own buffered readers and ship buffer-overrun bugs (CVE category). Rust gives a battle-tested type. | C, any low-level language |
| `read_line` (vs `lines()`) | Stream APIs that return iterators often consume the underlying reader, preventing mixed-mode reads. Rust's ownership system makes this visible at compile time. | All streaming I/O frameworks |
| `read_exact` | "I called read once and assumed I got the whole message" production bug. Type signature makes the count-known case explicit. | C, all stream protocols |
| `Content-Length: N\r\n\r\n` framing | TCP gives you bytes, not messages. Without framing, you have to guess where one message ends and the next begins. | All stream-based protocols |
| `Option<usize>` for content_length | Default `0` silently reads 0 bytes when the header is missing — a quiet failure. `Option` makes "header missing" an explicit error path. | Any language with sentinel-value defaults |

---

## A note on the smoke test

There's a `#[cfg(test)] mod tests` block at the bottom of `m1_read_one_message.rs` you might have noticed. It's a small regression check that verifies the parser still produces the right JSON when given a fixture Content-Length-framed message. Run it with:

```bash
cargo test --workspace --all-targets
```

The test syntax (`#[tokio::test]`, `TcpListener::bind`, the assertion macros) isn't covered yet — treat it the way you've been treating `#[tokio::main]`: trust me on it for now, we'll dig into how testing works in a dedicated chapter before we build the protocol crate's codec. The test exists so that if codelldb's response format ever drifts again, or if a future you refactors the parser, the regression check catches it before the lesson hangs in someone's terminal.

The test asserts on the chapter's *promise* (`success: true`, `command: initialize`, `type: response`) — not on how the parser is structured. That means you could rewrite `read_one_message` from scratch in any idiomatic shape you want and the test still passes. Tests should constrain behaviour, not creativity.

---

## See also

- ← [Chapter 04: Hello, adapter](04-hello-adapter.md)
- → Chapter 06: serde + typed protocols *(coming soon)*
- [Underlying milestone: M1 — Read one message](../implementation/tasks/M01-read-one-message.md)
- [`docs/issues/0002-codelldb-version-drift-rust-log.md`](../issues/0002-codelldb-version-drift-rust-log.md) — the version drift this chapter's matcher tolerates
- [`docs/reference/codelldb-quirks.md`](../reference/codelldb-quirks.md) — codelldb idiosyncrasies
