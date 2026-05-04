---
chapter: 7
session_id: M2-2
title: DAP transport and atomic seq
phase: A
estimated_time_minutes: 120
artifact: A reusable `DapTransport` struct that owns the codelldb subprocess + TCP stream + sequence counter, and a single generic `request<T, R>` method that handles every DAP command from now on. Real codelldb returns a real typed `Capabilities` struct via `cargo run --example m2_initialize`.
prerequisites:
  - Chapter 06 (serde-typed-protocols) — typed DAP shapes exist in `crates/dap/types.rs`
  - You're comfortable with the `serde_json::Value` walk and ready to never write one again
new_concepts:
  - The transport pattern — struct owns I/O state (child, stream, seq counter) + one generic method handles every protocol primitive
  - Generic methods with trait bounds (`<T: Serialize, R: DeserializeOwned>`) — extending yesterday's generic *types* to generic *methods*
  - `AtomicI64` for shared mutable counters — interior mutability through `&self`, hardware-level race-freedom
  - `thiserror::Error` derive — `#[error("...")]` for `Display`, `#[from]` for `?`-propagation, two independent jobs
  - Full-duplex demultiplexing — events vs responses on the same byte stream
related_milestone: docs/implementation/tasks/M02-initialize-handshake.md
---

# Chapter 07 — DAP transport and atomic seq

> Session ID: `M2-2` · Phase A · ~120 min · [Underlying milestone](../implementation/tasks/M02-initialize-handshake.md)

## What you'll learn

How to put yesterday's typed structs in motion behind **one reusable transport**. Today's tool: a struct that owns the I/O state (child process, TCP stream, sequence counter) and exposes a single generic method that handles **every** DAP command — initialize, launch, stackTrace, eval, all of them — by parameterising over the request body and response body types.

The deeper move: **for protocols with N primitives that share an envelope shape, prefer one generic method + N typed bodies over N specialised methods.** The variability moves into the type system where the compiler checks it.

Three new mechanics land in service of that pattern: generic methods with trait bounds (extending yesterday's generic *types*), `AtomicI64` for the sequence counter (interior mutability through `&self`, hardware-level race-freedom), and `thiserror::Error` for ergonomic typed errors that compose cleanly with the `?` operator.

## What you'll build

A `DapTransport` struct in `crates/dap/src/transport.rs`, a `TransportError` enum, and an `m2_initialize` example that spawns real codelldb, sends a real typed `InitializeArgs`, parses the typed `Capabilities` body off the wire, and pretty-prints it.

> By the end of this chapter, running:
>
> ```bash
> cargo run --example m2_initialize
> ```
>
> will print:
>
> ```
> Capabilities {
>     supports_configuration_done_request: true,
>     supports_function_breakpoints: true,
>     supports_conditional_breakpoints: true,
> }
> ```
>
> And the call site for ANY future DAP command becomes a single line:
>
> ```rust
> let body: SomeResponseType = transport.request("commandName", &args).await?;
> ```
>
> One method. Every DAP command. From here through the rest of the project.

## Before you start

**Prior knowledge assumed:**

- Chapter 06 is shipped — `Capabilities`, `DapResponse<R>`, `InitializeArgs` exist in `crates/dap/src/types.rs`.
- You've used a generic `<R>` on a struct before (yesterday's `DapResponse<R>`).
- You've encountered async/`?`-propagation enough to be unfazed by an `async fn` returning `Result<T, E>`.

**Setup state required:**

```bash
cargo test -p lazydap-dap     # 3 tests pass from chapter 06
which codelldb                # the wrapper script from chapter 04
```

If either fails, fix it before continuing.

---

## Surface your model first

Before we touch any Rust:

> 🤔 **Q:** In TypeScript or Python, suppose you're writing an HTTP client class to talk to a JSON API. Sketch the *shape* of it:
>
> 1. What *state* lives in the class (constructor params + fields)?
> 2. What's the signature of a generic `request` method that handles every endpoint? (e.g., one that can fetch a `User`, a `List<Post>`, an `Order`, etc., all via the same method.)
> 3. Where does the auto-incrementing request ID come from — class field, parameter, generated per-call?
> 4. How does the *caller* tell the method what response type to deserialise into?

<details>
<summary>Click after you've answered</summary>

A typical TS sketch:

```typescript
class HttpClient {
  private baseUrl: string;
  private auth: Token;
  private requestId: number = 0;     // monotonic counter

  constructor(baseUrl: string, auth: Token) { ... }

  async request<R>(path: string, body: any): Promise<R> {
    const id = ++this.requestId;
    const response = await fetch(this.baseUrl + path, ...);
    return response.json() as R;
  }
}
```

State in the class: connection details (URL, auth, etc.). One generic method `request<R>(...)`. Auto-incrementing ID is a stateful field. Caller passes `R` as a type argument.

**One thing most readers miss**: the *connection itself*. Fetch/axios are stateless because they create a fresh connection per call (HTTP/1.1's connection-per-request, modulo keep-alive). For *persistent-connection* transports — WebSocket clients, gRPC clients, our DAP transport — the **socket / stream** is also state. Once you've connected to codelldb, you keep that TCP stream open across many requests.

</details>

---

## Where DapTransport sits on the spectrum

A meta-question worth asking before writing code: "is this fetch/axios, or is it a Stripe SDK?"

It's **the middle**. Look at the spectrum:

| Tool | What it knows about the wire |
|---|---|
| `fetch` / axios | Generic HTTP. You compose URL, method, headers, body per call. The library knows nothing about *your* protocol. |
| **DapTransport (today)** | DAP-specific. Knows the `Content-Length: N\r\n\r\n` framing. Knows DAP's request/response envelope. Knows how to correlate a response to the request that triggered it (match `request_seq`). One generic method, parameterised by the typed bodies you defined yesterday. |
| Stripe SDK / Twilio SDK | One method per API endpoint. `stripe.charges.create(...)`, `stripe.subscriptions.list(...)`. The SDK encodes *every* endpoint as a typed method. |

DapTransport is **protocol-aware** (framing, seq, correlation) with **one generic method, not one method per command**. We're not writing `transport.initialize(args)`, `transport.launch(args)`, `transport.stack_trace()`. We're writing `transport.request("initialize", &args)` and parameterising via the *types* (`InitializeArgs`, `Capabilities`, etc.) — those carry the per-command shape.

DAP has ~30 commands. A method-per-command SDK would be tedious to write, churns when the spec evolves, and Rust's generics + serde derive let us pay that cost *once* and reuse it across every command. The leverage moves from the transport into the type system.

> **Pocket this as a design principle:** when you have N protocol primitives that all share the same envelope shape (request → seq + body, response → seq + result body, errors → uniform), prefer **one generic method + N typed bodies** over **N specialised methods**. The variability moves into the type system where the compiler can check it for you.
>
> The trade-off: you lose IDE autocomplete on per-command method names. Stripe SDK lets you type `stripe.charges.` and see every endpoint; our generic transport requires you to know command names by string. For 30 commands and a fast-evolving spec, scalability beats discoverability.

---

## Concept 1 — error types with `thiserror`

Before we write the transport, we need an error type. Why not just `anyhow::Error` (which the m1 example used)?

**`anyhow` is for application code.** It boxes any error into a single dynamic type. Great for binaries and examples; lossy for libraries because callers can't pattern-match on what went wrong.

**Library code wants a typed enum.** Each failure mode is a variant: I/O failed, JSON parse failed, the adapter said "no", we got malformed framing. Callers can `match` on it and do something specific (retry on I/O, give up on Dap-level error). `crates/dap` is a library; it gets the typed treatment.

**`thiserror`** is the derive macro for this. Same shape as `serde_derive` — proc macro reads your enum and generates the boilerplate (`impl Error`, `impl Display`, conversion `From` impls). You write the *shape*; the macro writes the noise.

The boilerplate eliminated:

```rust
// What you'd write WITHOUT thiserror — the noise.
pub enum TransportError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Header(String),
    AdapterExited,
    Dap(String),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Json(e) => write!(f, "json: {e}"),
            // ... etc, one match arm per variant
        }
    }
}
impl std::error::Error for TransportError { /* ... */ }
impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self { Self::Io(e) }
}
impl From<serde_json::Error> for TransportError {
    fn from(e: serde_json::Error) -> Self { Self::Json(e) }
}
// More From impls per variant you want to auto-convert.
```

40+ lines of mechanical glue. With thiserror, the same thing:

```rust
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid header: {0}")]
    Header(String),

    #[error("adapter exited unexpectedly")]
    AdapterExited,

    #[error("dap error: {0}")]
    Dap(String),

    #[error("port parse: {0}")]
    PortParse(#[from] std::num::ParseIntError),

    #[error("adapter did not announce a port on stderr")]
    NoPortFromAdapter,
}
```

Two attributes do the work, **and they do non-overlapping jobs**:

| Attribute | Trait it generates | When it kicks in |
|---|---|---|
| `#[error("...")]` | `Display` (and `Error::source` plumbing) | Printing the error: `eprintln!("{err}")`, log macros |
| `#[from]` | `From<Inner> for Outer` | Error propagation: `inner_call()?` |

> 🔮 **Predict:** Look at the `Io` variant — `Io(#[from] std::io::Error)`. After this derive, you can write:
>
> ```rust
> async fn read_n_bytes(stream: &mut TcpStream) -> Result<Vec<u8>, TransportError> {
>     let mut buf = vec![0u8; 100];
>     stream.read_exact(&mut buf).await?;  // <-- the ? on a std::io::Error
>     Ok(buf)
> }
> ```
>
> `read_exact` returns `Result<(), std::io::Error>`. The function returns `Result<Vec<u8>, TransportError>`. The `?` operator needs to convert one error type to the other.
>
> **What does `#[from]` generate that makes `?` compile here?** Specifically: which trait, on which type, with what method?

<details>
<summary>Click after you've predicted</summary>

`?` desugars to roughly:

```rust
match x {
    Ok(v) => v,
    Err(e) => return Err(From::from(e)),  // <-- ? calls From automatically
}
```

So for `?` to compile, the rule is: **the error type produced by the right-hand expression must implement `From<...>` *for* the function's return error type.**

In the example:
- `read_exact(...).await?` produces `std::io::Error` on failure
- The function returns `Result<_, TransportError>`
- For `?` to compile, you need `impl From<std::io::Error> for TransportError`

That's exactly what `#[from]` generates. **Trait: `From<std::io::Error>`. Impl on: `TransportError`. Method: `fn from(e: std::io::Error) -> TransportError` returning `TransportError::Io(e)`.**

`Display` doesn't enter into this path. Display only fires when someone *chooses* to print the error — `eprintln!("{err}")`. `?` only knows about `From`. They're independent concerns.

**Common confusion to pre-empt:** `Display` and `From` are completely independent. `From` does NOT invoke `Display`. The former is for type conversion (`?`-propagation); the latter is for human-readable output (`println!`/`eprintln!`/log macros). Each attribute generates its own trait impl; neither calls the other.

</details>

---

## Concept 2 — the transport struct

Three fields:

```rust
pub struct DapTransport {
    child: Child,                       // codelldb subprocess
    stream: BufReader<TcpStream>,       // the persistent TCP connection
    seq: AtomicI64,                     // monotonic request counter
}
```

`Child` is from `tokio::process` — it's the handle to the child process we spawned. We need to keep it alive for the lifetime of the transport (otherwise the OS kills it) and have a way to clean it up on `shutdown`.

`BufReader<TcpStream>` is the same wrapping you saw in chapter 05. The BufReader lives on the struct (across calls to `request`) so it can buffer bytes between calls. Critically: the *same* BufReader instance handles every read. Going back to the raw `TcpStream` would lose buffered bytes (chapter 05's footgun, surfacing again at the struct level).

`AtomicI64` is the meaty one. We'll get to *why* atomic in a moment — first the spawn function:

```rust
impl DapTransport {
    pub async fn spawn(adapter_path: &str) -> Result<Self> {
        let mut child = Command::new(adapter_path)
            .arg("--port")
            .arg("0")
            .env("RUST_LOG", "debug")        // codelldb's port log line is at debug
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stderr = child.stderr.take().expect("stderr piped");
        let mut lines = BufReader::new(stderr).lines();

        let mut port: Option<u16> = None;
        while let Some(line) = lines.next_line().await? {
            tracing::debug!(target: "dap.adapter.stderr", "{line}");
            if let Some((_, rest)) = line.split_once("Listening on ") {
                let port_str = rest
                    .strip_prefix("port ")
                    .unwrap_or_else(|| rest.rsplit(':').next().unwrap_or(rest));
                port = Some(port_str.trim().parse()?);
                break;
            }
        }
        let port = port.ok_or(TransportError::NoPortFromAdapter)?;

        // Drain rest of stderr in the background so codelldb doesn't block.
        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "dap.adapter.stderr", "{line}");
            }
        });

        let stream = TcpStream::connect(("127.0.0.1", port)).await?;
        Ok(Self {
            child,
            stream: BufReader::new(stream),
            seq: AtomicI64::new(1),
        })
    }
}
```

This is a clean factoring of chapter 05's `m1_read_one_message.rs` spawn helper into a struct constructor. Three things worth pointing at:

1. **`.env("RUST_LOG", "debug")`** — codelldb's `"Listening on HOST:PORT"` line is logged at *debug* level. Without this env var, codelldb is silent on stderr and the spawn loop blocks forever. (You'll trip on this if you forget it. The compiler-conversation section below has the diagnosis.)
2. **`.kill_on_drop(true)`** — when `DapTransport` is dropped (out of scope), tokio kills the child process. No zombie processes.
3. **`tokio::spawn` for the stderr drain** — without this background task, codelldb's stderr buffer fills (~64 KiB kernel buffer) and the adapter blocks on its next log write. Same pattern as chapter 04.

---

## Concept 3 — the generic request method, atomic seq edition

The signature first, so you see where we're going:

```rust
pub async fn request<T: Serialize, R: DeserializeOwned>(
    &mut self,
    command: &str,
    args: &T,
) -> Result<R> {
    // 1. Pick a sequence number (atomic increment)
    // 2. Build + frame + send the outbound envelope
    // 3. Read messages until we see the matching response (skip events)
    // 4. Deserialise the response body into R, propagate errors
}
```

Two type parameters: `T` is the args type (the caller's `InitializeArgs`, `LaunchArgs`, etc.); `R` is the response body type (`Capabilities`, `StackTraceBody`, etc.). Their bounds:

- **`T: Serialize`** — serde must be able to convert T to JSON.
- **`R: DeserializeOwned`** — serde must be able to construct R from JSON, with no borrowed lifetimes hanging on (it owns its data).

`&str` for `command` (we never need it after the call) and `&T` for `args` (we only borrow long enough to serialise — no need to take ownership). Both compile away to zero overhead.

`DeserializeOwned` is a slight Rust wrinkle worth flagging now and forgetting: it means "deserialises into an owned value, no borrowed lifetimes." We need *Owned* because the JSON bytes that produce R get freed before R is returned; R can't borrow from those bytes. The plain `Deserialize` trait allows borrowing from the input bytes, which is faster but lifetime-tangled. **For 95% of cases (including ours), use `DeserializeOwned`.**

### The seq counter — why atomic?

The first line of the body uses `AtomicI64::fetch_add`:

```rust
let seq = self.seq.fetch_add(1, Ordering::SeqCst);
```

> 🔮 **Predict — two small questions:**
>
> 1. What value does `fetch_add` return into `seq`? Is it the **previous** value of `self.seq` (before adding 1), or the **new** value (after)?
> 2. We declared `seq: AtomicI64` instead of `seq: i64`. Why bother — given `request` takes `&mut self`, we already have exclusive access to all our fields. What does atomic buy us that plain `i64` mutation doesn't?

<details>
<summary>Click after you've predicted</summary>

**Q1**: `fetch_add` returns the **previous** value, then adds. Like `i++` in C/JS, not `++i`. The name says it: "fetch current, then add" — what gets fetched is what comes back. Mental model: we're *claiming a slot*. Initial value 1, first `fetch_add(1)` returns 1 (and leaves counter at 2), next returns 2, etc.

**Q2**: This is the load-bearing question — let's go from the bottom.

**The problem in plain C** (anchor on a language you may be learning right now):

```c
int counter = 0;

void thread_func() {
    counter = counter + 1;   // does this look safe? It isn't.
}
```

`counter = counter + 1` is **not one CPU instruction**. It's three:

1. **Read** counter from memory into a register.
2. **Add** 1 to the register.
3. **Write** the register back to memory.

If two threads run this at once:

```
Time   Thread A         Thread B         Memory
---    -------          -------          ------
1      Read counter (5)                  5
2                       Read counter (5) 5
3      Compute 5+1 = 6                   5
4                       Compute 5+1 = 6  5
5      Write 6                           6
6                       Write 6          6
```

You wanted `counter == 7` (incremented twice). You got `counter == 6`. **One increment lost.** The most common concurrency bug in C/C++.

**"Atomic" means the operation happens as one indivisible step.** Modern CPUs have special instructions (`LOCK XADD` on x86, `LDAREX/STREX` on ARM) that do read-modify-write as one uninterruptible step. C11 added `_Atomic int` to expose them; Rust's `AtomicI64` is the same idea. `self.seq.fetch_add(1, ...)` compiles to that single instruction. Two threads calling it at the same time produce two different return values (one gets 5, one gets 6), counter ends at 7. **Increment never lost, by hardware guarantee.**

**Now the `&self` vs `&mut self` part.**

Rust's normal rule: **mutation requires `&mut self`** (exclusive borrow). The borrow checker enforces this at compile time to prevent races — if only ONE thing can mutate at a time, races are impossible.

But this is conservative. It rules out cases that ARE safe — like our seq counter, where the *hardware itself* prevents races via the atomic instruction. So Rust gives you an escape hatch: types that mutate through `&self` (immutable reference) by relying on a stronger safety mechanism. AtomicI64's signature shows this:

```rust
fn fetch_add(&self, ...) -> i64
//           ^^^^^ &self, NOT &mut self
```

Multiple references to the same `AtomicI64` can call `fetch_add` simultaneously — the hardware sorts out the ordering. With a plain `i64`, only ONE thing could mutate at a time (because mutation requires `&mut`, which requires exclusive access).

**Why we care for our transport, in concrete terms.**

**Today** (single caller): `request` takes `&mut self`. There's only one caller. We don't strictly need atomics — a plain `i64` would work. **We're using AtomicI64 anyway because it's free and keeps options open.**

**Tomorrow** (if we want shared ownership): suppose we want one task to be in the middle of `request("launch", ...)` while another reads the current `seq` for logging. With plain `i64`, this needs `&mut` for the increment, which **rules out the read-while-incrementing case** — the borrow checker rejects it. With `AtomicI64`, both can hold `&AtomicI64` and operate concurrently.

**Cost is essentially zero.** On x86, fetch_add is a single locked instruction (~10-50 ns of fence cost). Invisible at our call rate.

**One-line summary:** atomic = "the read-and-modify-and-write is a single CPU instruction that no other thread can interleave with." `&self`-mutation falls out of that hardware guarantee.

</details>

The `Ordering::SeqCst` argument: trust me on it for now. It's the strongest memory ordering (sequentially consistent) and always-correct — sometimes slower than necessary but never wrong. Acquire/Release/Relaxed lands when we hit a place where weaker ordering is *provably* safe. Deferred-load.

### The outbound envelope

```rust
let outbound = serde_json::json!({
    "seq": seq,
    "type": "request",
    "command": command,
    "arguments": args,    // T: Serialize → serde calls T's Serialize impl here
});
let body = serde_json::to_vec(&outbound)?;
let header = format!("Content-Length: {}\r\n\r\n", body.len());
self.stream.get_mut().write_all(header.as_bytes()).await?;
self.stream.get_mut().write_all(&body).await?;
self.stream.get_mut().flush().await?;
tracing::debug!(target: "dap.send", seq, command, "request");
```

One subtle thing worth flagging: `"arguments": args` inside `json!`. The macro takes any `T: Serialize` value and recursively serialises it. So when the caller passes `&InitializeArgs { ... }`, that whole typed struct (with all its `rename_all` and per-field `rename` annotations) flows through into the right wire shape automatically. **The typed structs from yesterday and the json! macro compose without us having to glue anything.**

`self.stream.get_mut()` reaches inside the `BufReader<TcpStream>` to get back the underlying `&mut TcpStream` for writing. (BufReader buffers *reads* only; writes go straight through.)

### The response read — full-duplex demux

The response-reading half is structured as a **`loop { ... }`**, not a single read.

> 🔮 **Predict:** Why a loop? We just sent one request — surely we expect one response and we're done? What might come back from the adapter that *isn't* the response we want?

<details>
<summary>Click after you've predicted</summary>

**The reason for the loop: full-duplex event push.** DAP is a *full-duplex* protocol — both sides can send messages whenever. Two message types come *inbound* on the same channel:

1. **Responses** (`"type": "response"`) — matched 1:1 to a request you sent, correlated by `request_seq`.
2. **Events** (`"type": "event"`) — *server-pushed*. The adapter sends these whenever it wants. No request triggered them. Examples: `output` events when the debuggee prints; `stopped` events when it hits a breakpoint; `initialized` event right after `initialize` saying "I'm ready for `setBreakpoints` now."

Between sending your `initialize` request and receiving its response, codelldb *might* send an `output` event with debug logging. During `launch`, you'll get a *stream* of `process`, `thread`, `output`, `stopped` events before the launch response arrives.

**Our `request` method has to demultiplex events from responses on read.** That's what the loop does.

A common wrong intuition is request-multiplexing (HTTP/2-style: many concurrent requests in flight, responses come back tagged with stream IDs, you sort by ID). DapTransport ISN'T that — we send one request at a time and wait. The loop isn't there to disambiguate *our* requests; it's there to skip *the adapter's pushed events*.

**Pocket this protocol distinction:** request-response (HTTP/1.1) → server only talks when you ask. Full-duplex (DAP, LSP, WebSocket, gRPC streaming) → server can push any time. The latter ALWAYS needs a demultiplexing read loop, because events and responses share the same byte stream.

</details>

The full read loop:

```rust
loop {
    let body = self.read_message_body().await?;
    let value: serde_json::Value = serde_json::from_slice(&body)?;
    let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "response" => {
            let resp: DapResponse<R> = serde_json::from_slice(&body)?;
            if resp.request_seq != seq {
                tracing::warn!(
                    request_seq = resp.request_seq,
                    expected = seq,
                    "out-of-order response, ignoring",
                );
                continue;
            }
            if !resp.success {
                return Err(TransportError::Dap(resp.message.unwrap_or_default()));
            }
            return resp
                .body
                .ok_or_else(|| TransportError::Dap("empty response body".into()));
        }
        "event" => {
            let event_name = value.get("event").and_then(|v| v.as_str()).unwrap_or("?");
            tracing::debug!(target: "dap.recv.event", event_name, "ignoring event");
        }
        other => {
            tracing::warn!(kind = other, "unknown message type");
        }
    }
}
```

Two layers worth distinguishing:

| Layer | Why | Code |
|---|---|---|
| **Outer loop** | Demultiplex events from responses on a full-duplex channel | `loop { read; match type }` |
| **request_seq check** | Defensive correlation — protect against weird adapter behaviour or races | `if resp.request_seq != seq { continue }` |

For today, we **log-and-skip** events. M3 (chapter 08) wires them to a channel so the caller can consume them properly.

The `read_message_body` helper is essentially chapter 05's parser, lifted onto `self`:

```rust
async fn read_message_body(&mut self) -> Result<Vec<u8>> {
    let mut header_buf = String::new();
    let mut content_length: Option<usize> = None;
    loop {
        header_buf.clear();
        let n = self.stream.read_line(&mut header_buf).await?;
        if n == 0 {
            return Err(TransportError::AdapterExited);
        }
        let trimmed = header_buf.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(v) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                v.trim()
                    .parse()
                    .map_err(|_| TransportError::Header(trimmed.into()))?,
            );
        }
    }
    let len = content_length.ok_or_else(|| TransportError::Header("no Content-Length".into()))?;
    let mut body = vec![0u8; len];
    self.stream.read_exact(&mut body).await?;
    Ok(body)
}
```

Same `read_line` headers + `read_exact` body pattern. Same BufReader-instance discipline. The only material change: errors are typed (`TransportError::AdapterExited`, `TransportError::Header`) instead of `anyhow!(...)` — that's `thiserror` paying off.

---

## Try it yourself — round 3

Write `crates/daemon/examples/m2_initialize.rs`. It should:

1. Initialise tracing (`tracing_subscriber::fmt::init()` at the top of `main`).
2. Spawn the transport against `codelldb`.
3. Send an `initialize` request with `InitializeArgs::default()` as the args.
4. Pretty-print the resulting `Capabilities`.
5. Cleanly shut down (or rely on `kill_on_drop`).

The new API surface from `lazydap_dap`:

```rust
DapTransport::spawn("codelldb").await -> Result<DapTransport>
transport.request::<T, R>(command, args).await -> Result<R>
transport.shutdown().await -> Result<()>
```

Two things that'll trip you up if you hurry:

1. **The `request` method is generic over `R`.** The compiler can't infer R from the inputs alone — so the *binding* needs a type annotation: `let caps: Capabilities = transport.request(...).await?;`. Without it, you'll get a "type annotations needed for `R`" error.
2. **Tracing is opt-in via `RUST_LOG`.** To see the `tracing::debug!` and `tracing::warn!` calls actually print, run with `RUST_LOG=dap=debug,lazydap_dap=debug cargo run --example m2_initialize`.

Pretty-print with `println!("{caps:#?}")` (uses the Debug impl).

Write it. Run it. If it hangs, read the next section.

---

## Compiler-and-runtime conversation — the codelldb-quiet-without-RUST_LOG hang

The most common runtime failure on this exercise is **silent hang** — your binary launches, no tracing output appears, and it never returns. Symptom:

```bash
$ cargo run --example m2_initialize
   Compiling lazydap-dap v0.1.0 ...
    Finished dev profile target(s) in 1.24s
     Running `target/debug/examples/m2_initialize`
[hangs forever, no output]
```

**The root cause:** codelldb's `"Listening on 127.0.0.1:NNNN"` line is logged at *debug* level. **Without `RUST_LOG=debug` set in codelldb's environment**, the adapter is silent on stderr. Our `spawn` loop blocks forever on `lines.next_line().await`, waiting for a line that never comes.

That's why the spawn function sets `.env("RUST_LOG", "debug")` on the Command. If you wrote your own spawn helper (or modified the existing one to pass through environment variables you control), check that codelldb is getting `RUST_LOG=debug`. Without it, hang.

**Diagnostic moves when you hit a hang:**

1. **Read the tracing output.** If you initialised `tracing_subscriber` and see *zero* tracing lines, you're hung *before* any `tracing::debug!` call fires. That points at the spawn loop's `next_line()`.
2. **Run codelldb directly.** `RUST_LOG=debug codelldb --port 0` — does it print "Listening on..."? If not, your codelldb install is broken (likely the symlink-not-script issue from chapter 04).
3. **`pgrep codelldb`** to check for zombies. `kill_on_drop(true)` should prevent these, but a hang during spawn means the cleanup never ran.
4. **Compare to the m1 example.** If `cargo run --example m1_read_one_message` works but `m2_initialize` doesn't, the difference is in the transport spawn function. Diff them.

This is the same *class* of issue as chapter 05's version-drift hang (matcher format mismatch). General principle: **when async I/O hangs, ask "what is the read waiting for?" and "did the writer actually produce that?"** Run the writer directly to see what it actually emits; compare to your matcher's assumptions.

---

## A real-world hardening — `#[serde(default)]` on `Capabilities`

A small change to chapter 06's `Capabilities` lands in this chapter: a struct-level `#[serde(default)]` annotation.

```rust
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]   // <-- added: "default"
pub struct Capabilities {
    pub supports_configuration_done_request: bool,
    pub supports_function_breakpoints: bool,
    pub supports_conditional_breakpoints: bool,
}
```

The reason: real codelldb might omit any of those three fields across versions. Without `#[serde(default)]`, a missing field is a hard error. With it, missing fields land as their type's `Default` (`false` for `bool`).

This is the **other** way to opt out of the missing-field error — the alternative I flagged in chapter 06 next to `Option<T>`. Choose between them based on semantics:

- `Option<T>` for "may genuinely not exist" (e.g., the `message` field on a DapResponse — only present on errors)
- `#[serde(default)]` for "missing means use a sensible default value" (e.g., capability flags — missing = "feature not supported" = `false`)

Both are real. Both are used in real serde codebases. For *receiving* protocol data from a partner of unknown version, `#[serde(default)]` plus the `Default` derive is the robust default.

---

## What you can run now

```bash
RUST_LOG=dap=debug,lazydap_dap=debug cargo run --example m2_initialize
```

The trace shows the whole protocol dance:

```
[INFO  codelldb] Loaded liblldb.dylib version="lldb version 20.1.4-codelldb"
[DEBUG codelldb] Listening on 127.0.0.1:62402
[DEBUG codelldb] New debug session
dap.send: request seq=1 command="initialize"
--> {"arguments":{...},"command":"initialize","seq":1,"type":"request"}
<-- {"seq":1,"type":"response","request_seq":1,"success":true,...,"body":{...25 capability flags...}}
Capabilities {
    supports_configuration_done_request: true,
    supports_function_breakpoints: true,
    supports_conditional_breakpoints: true,
}
```

**Ladder check:**

- WS-1 to WS-3: workspace + binary scaffold (no protocol).
- M0-1: spawn codelldb (no JSON yet).
- M1-1: read one framed message (one-shot, untyped, hand-rolled per call).
- M2-1: typed structs (parse a fixture into them; not yet on the wire).
- **M2-2 (this chapter): typed structs + reusable transport. Real codelldb. One generic method that handles every DAP request from now on.**

Every future command — `launch`, `stackTrace`, `eval`, `setBreakpoints`, all of them — is now a one-line call. Define the typed args struct, define the typed response body, call `transport.request(command, &args)`. The transport is *done* until M3 (event streaming).

---

## Teach-back

Three questions in your own words. Stumble means re-read the relevant section.

> 📣 **Q1:** Frame the design choice we made: one generic `request<T, R>` method instead of one method per DAP command (no `transport.initialize(args)`, `transport.launch(args)`, etc.). Why is the generic-method approach better here, and what would a method-per-command client (like a Stripe SDK) buy you that we're giving up?

> 📣 **Q2:** Walk through what happens when `self.stream.read_exact(&mut body).await?` returns `Err(io_err)`. What trait gets invoked, what does it return, and where does `#[from]` fit into making this compile?

> 📣 **Q3:** Why does `request` use a `loop`, and what does it filter out? Bonus — in a strict request-response protocol like HTTP/1.1, would we still need the loop? Why or why not?

---

## Pain anchors covered

| New construct | The pain it solves | In which language |
|---|---|---|
| `thiserror::Error` derive | C's "return -1 and set errno" + manual `From`/`Display`/`Error` impls. C++'s exception hierarchies. JS's "throw whatever you feel like." Rust's typed-error-as-data + thiserror gives ergonomic typed errors with auto-generated `?`-conversion. | C, JS, Rust without thiserror |
| `<T: Serialize, R: DeserializeOwned>` on methods | Java's "interface that all my types must implement" or TS's class generics. Rust's trait bounds let you require capabilities of generic types at compile time. | Generic in any typed language; bounds are Rust-specific syntax |
| `AtomicI64` interior mutability | C's data races (read-modify-write on a shared int across threads loses increments). Java's `synchronized` blocks. Python's GIL hiding the issue. Rust's atomic types make race-freedom a hardware-level guarantee while letting you mutate through `&self`. | C, C++, Java, Rust without atomics |
| Full-duplex demux loop | Treating event streams like request-response (the wrong mental model from HTTP). Full-duplex protocols require a read loop that can route events to one place and responses to another. WebSocket clients hit this constantly. | Any full-duplex protocol — DAP, LSP, WebSocket, gRPC streaming |
| `#[serde(default)]` for protocol resilience | Hard-failing on missing fields when partner versions drift. Pydantic / Zod's `.optional()` + `.default()`. Serde's `#[serde(default)]` lets missing fields use the type's `Default`, which for booleans is `false`. | Any protocol with version drift |

---

## A note on the smoke test

This chapter does **not** ship with a unit-test smoke test, by design. The chapter's promise — "real codelldb, real DAP wire, real typed Capabilities back" — is irreducibly integration-flavoured. A unit test would require either:

- Refactoring `DapTransport` to accept a generic `R: AsyncRead + AsyncWrite + Unpin` so we can swap in a mock stream — adds a generic parameter the learner doesn't need to see, violates rule 8 (defer load multipliers).
- A test-only constructor that bypasses `spawn` — adds API surface that doesn't serve the chapter's promise.

Instead, **the example file `m2_initialize.rs` IS the smoke test.** It lives in the repo, gets built by CI (`cargo build --workspace --all-targets`), and a regression in the transport API will fail compilation. The behavioural verification (it actually talks to codelldb and produces a valid Capabilities) requires running it with codelldb installed — same trade-off as chapter 04.

When chapter 08 (M3) introduces event streaming, the testable invariants change (event channel correctness can be unit-tested without codelldb), and the smoke-test status will change with it.

---

## See also

- ← [Chapter 06: Serde and typed protocols](06-serde-typed-protocols.md)
- → Chapter 08: Event streaming and tagged enums *(coming soon)*
- [Underlying milestone: M2 — Initialize handshake](../implementation/tasks/M02-initialize-handshake.md)
- [`docs/issues/0002-codelldb-version-drift-rust-log.md`](../issues/0002-codelldb-version-drift-rust-log.md) — the codelldb logging quirk this chapter's `.env("RUST_LOG", ...)` works around
- [`thiserror` documentation](https://docs.rs/thiserror/) — full attribute reference
- [Rust atomics chapter in *The Rustonomicon*](https://doc.rust-lang.org/nomicon/atomics.html) — for when you're ready to dig into memory ordering
