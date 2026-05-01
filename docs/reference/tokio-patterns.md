# tokio patterns

Quick reference for the async patterns lazydap leans on. Full docs at [tokio.rs](https://tokio.rs).

## Spawning the runtime

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ...
}
```

`#[tokio::main]` defaults to multi-threaded. For lazydap, fine — daemon multi-threading is genuinely needed for parallel client connections + adapter I/O.

For tests:

```rust
#[tokio::test]
async fn it_works() { ... }
```

## Process spawning

```rust
use tokio::process::Command;
use std::process::Stdio;

let mut child = Command::new("codelldb")
    .arg("--port").arg("0")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .stdin(Stdio::piped())
    .kill_on_drop(true)
    .spawn()?;

let stdout = child.stdout.take().expect("piped");
let stderr = child.stderr.take().expect("piped");
```

`kill_on_drop(true)` is critical. Without it, child processes leak when their owner errors out.

## Reading process output

```rust
use tokio::io::{AsyncBufReadExt, BufReader};

let mut lines = BufReader::new(stderr).lines();
while let Some(line) = lines.next_line().await? {
    tracing::debug!(target: "adapter.stderr", "{line}");
}
```

For binary or framed protocols, use `read_exact`:

```rust
use tokio::io::AsyncReadExt;

let mut buf = vec![0u8; len];
stream.read_exact(&mut buf).await?;
```

**Always drain stderr in a separate task.** If stderr buffer fills, the child process blocks (cannot write more). This bites silently.

```rust
tokio::spawn(async move {
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::debug!(target: "adapter.stderr", "{line}");
    }
});
```

## Channels

### `mpsc` (multi-producer, single-consumer)

```rust
let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();

// Sender:
tx.send(msg)?;

// Receiver:
while let Some(msg) = rx.recv().await {
    // handle
}
```

For backpressure use `mpsc::channel(capacity)` (bounded).

For lazydap: use `unbounded` for IPC events (we control producer rate), `bounded` for output buffers (cap memory).

### `broadcast` (multi-consumer, lossy if slow)

```rust
let (tx, _) = tokio::sync::broadcast::channel::<Event>(1024);

// Subscribe (each subscriber gets its own receiver):
let mut rx = tx.subscribe();

// Send:
tx.send(event)?;       // returns Err if no subscribers; ignore

// Receive:
match rx.recv().await {
    Ok(event) => handle(event),
    Err(broadcast::error::RecvError::Lagged(n)) => {
        // We dropped `n` messages because we were too slow.
        warn!(?n, "lagged");
    }
    Err(broadcast::error::RecvError::Closed) => break,
}
```

Use for daemon → multiple clients: each client subscribes; lagging clients lose old events.

### `watch` (single value, latest-wins)

```rust
let (tx, mut rx) = tokio::sync::watch::channel(false);

tx.send(true)?;        // change

// Wait for change:
rx.changed().await?;
let value = *rx.borrow();
```

Use for shutdown signals, single-value-with-history-doesn't-matter state.

### `oneshot` (single use, request/response)

```rust
let (tx, rx) = tokio::sync::oneshot::channel::<Result<Foo>>();

// Producer:
tx.send(result).ok();

// Consumer:
let value = rx.await??;
```

Use for "send a request, wait for the response" patterns. The DAP transport's pending-requests map uses one `oneshot` per outstanding request.

## select!

Multiplexed waits.

```rust
tokio::select! {
    Some(input) = input_rx.recv() => {
        handle_input(input);
    }
    Some(event) = ipc_rx.recv() => {
        handle_ipc_event(event);
    }
    _ = tick.tick() => {
        on_tick();
    }
    _ = shutdown_rx.changed() => {
        break;
    }
}
```

Branches that aren't ready don't run; first ready branch wins. Cancellation-safe — futures that didn't complete are dropped cleanly.

**Gotcha:** `select!` evaluates the future on each iteration, so `read_line` on a `BufReader` may lose buffered data if not careful. Use `Pin<&mut Future>` for re-entrant futures, or guard with `tokio::pin!`.

## Timeouts

```rust
use tokio::time::{timeout, Duration};

match timeout(Duration::from_secs(30), some_future).await {
    Ok(Ok(value)) => value,
    Ok(Err(e)) => return Err(e),
    Err(_) => {
        // Timed out
        return Err("timeout".into());
    }
}
```

For repeated polling with overall budget:

```rust
let deadline = Instant::now() + Duration::from_secs(30);
while Instant::now() < deadline {
    let remaining = deadline.saturating_duration_since(Instant::now());
    match timeout(remaining, future).await {
        Ok(result) => return Ok(result),
        Err(_) => break,
    }
}
```

`--wait` in lazydap uses this pattern.

## Mutex and RwLock

```rust
use tokio::sync::{Mutex, RwLock};

let shared = Arc::new(RwLock::new(SessionState::default()));

// Read:
let state = shared.read().await;
// state is RwLockReadGuard

// Write:
let mut state = shared.write().await;
state.frames.push(frame);
```

Prefer `RwLock` over `Mutex` when reads outnumber writes (typical for state caches).

**Gotcha:** holding a guard across an `.await` is fine but blocks anyone waiting on the lock for the duration. Don't await with a guard held unless you have to.

## Task spawning

```rust
let handle = tokio::spawn(async move {
    do_work().await
});

// Wait:
let result = handle.await?;
```

For tasks that should die when the parent drops:

```rust
let handle = tokio::spawn(async move { ... });
// ...
handle.abort();   // cancel
```

For long-running tasks, use `tokio::task::JoinSet` to manage many handles together:

```rust
let mut set = JoinSet::new();
for task in tasks {
    set.spawn(async move { run(task).await });
}
while let Some(result) = set.join_next().await {
    handle(result);
}
```

## Cancellation

`tokio::select!` is cancellation-safe by default — the unfinished future is dropped. But:

- **Mid-state tasks may leak resources** if not careful. The DAP transport's pending requests map needs cleanup on cancellation.
- **`tokio_util::sync::CancellationToken`** for explicit cancellation:

```rust
let token = CancellationToken::new();
let token_clone = token.clone();

tokio::spawn(async move {
    tokio::select! {
        _ = do_long_work() => {}
        _ = token_clone.cancelled() => {
            // clean up
        }
    }
});

// Later:
token.cancel();
```

## Common pitfalls in lazydap context

### Don't pipeline DAP requests

The DAP adapter may serialise requests. Pipelining can deadlock. The daemon serialises execution requests per session — see [`/docs/blueprint/10-async-to-sync.md`](../blueprint/10-async-to-sync.md) D021.

```rust
// WRONG: send all at once
let f1 = adapter.request("continue", ...);
let f2 = adapter.request("step", ...);
let (r1, r2) = tokio::join!(f1, f2);

// RIGHT: queue, one at a time
let r1 = adapter.request("continue", ...).await?;
// only after r1 completes:
let r2 = adapter.request("step", ...).await?;
```

### Don't drop the read pump

If the read pump task is cancelled (because its owner dropped), DAP events stop arriving. Use `JoinHandle::abort_on_drop` (manually) or keep the pump alive for the entire transport's lifetime via `Arc`.

### Don't use `std::sync::Mutex` in async code

```rust
// WRONG:
let m: std::sync::Mutex<Foo> = ...;
let guard = m.lock().unwrap();
some_async_thing.await;        // BAD: blocking lock held across await

// RIGHT:
let m: tokio::sync::Mutex<Foo> = ...;
let guard = m.lock().await;
some_async_thing.await;        // FINE: tokio Mutex coexists with await
```

### Don't `block_on` inside an async context

If you're in a tokio task and call `Runtime::block_on`, you'll deadlock. Refactor the calling code to be `async fn`.

### Bounded vs unbounded channels

Default to `unbounded`. Switch to `bounded` only when you have a memory cap to respect or genuine backpressure semantics. Bounded channels can deadlock if a producer is also a consumer (cycle).

## Tracing

```rust
use tracing::{debug, info, warn, error, instrument};

#[instrument(skip(self), fields(session_id = %self.id))]
pub async fn handle(&self, req: Request) -> Result<Response> {
    debug!(?req, "handling request");
    // ...
    info!("done");
    Ok(response)
}
```

`#[instrument]` adds a span for the function's lifetime. Logs inside it are tagged with the session_id field.

Configure the subscriber once in `main`:

```rust
tracing_subscriber::fmt()
    .with_env_filter("lazydap=debug,info")
    .with_target(true)
    .init();
```

For JSON logs (background mode):

```rust
tracing_subscriber::fmt()
    .json()
    .with_writer(file)
    .init();
```

## Testing async code

```rust
#[tokio::test]
async fn it_works() {
    let result = some_async_fn().await;
    assert_eq!(result, expected);
}

#[tokio::test(start_paused = true)]
async fn timing_test() {
    // tokio::time::sleep is paused; advance manually:
    tokio::time::advance(Duration::from_secs(60)).await;
}
```

`start_paused = true` makes timeouts deterministic in tests.

## Resources

- [tokio.rs/tokio/tutorial](https://tokio.rs/tokio/tutorial) — start here
- [tokio.rs/tokio/topics/select](https://tokio.rs/tokio/topics/select) — `select!` deep dive
- [`tokio::sync` docs](https://docs.rs/tokio/latest/tokio/sync/index.html) — channel reference
- [Async Rust Book](https://rust-lang.github.io/async-book/) — language-level fundamentals
