# M18 — Second adapter (debugpy)

## What

`crates/adapter-debugpy/`. Debug a Python file end-to-end via lazydap. Same UX as codelldb. Multi-language unlock.

## Why

The first time you go from one adapter to two, you find every place you hardcoded codelldb assumptions. M18 surfaces and fixes them.

## How

### Step 1 — `crates/adapter-debugpy/`

```toml
[package]
name = "lazydap-adapter-debugpy"
...

[dependencies]
lazydap-core = { path = "../core" }
lazydap-dap = { path = "../dap" }
async-trait.workspace = true
tokio.workspace = true
```

`src/lib.rs`:

```rust
pub struct DebugPyAdapter {
    transport: DapTransport,
    capabilities: Capabilities,
}

#[async_trait]
impl DebugAdapter for DebugPyAdapter {
    fn kind(&self) -> AdapterKind { AdapterKind::DebugPy }

    async fn launch(&self, config: LaunchConfig) -> Result<Capabilities> {
        // debugpy spawn:
        //   debugpy-adapter [--port N]
        // OR
        //   python -m debugpy --listen 0 --wait-for-client <script.py>
        // We prefer debugpy-adapter (simpler, more like codelldb).
        ...
    }

    // ... request, events, disconnect mostly the same as codelldb wrapper
}
```

### Step 2 — Adapter discovery

`crates/config/src/discovery.rs`:

```rust
pub fn discover_debugpy() -> Option<PathBuf> {
    if let Some(p) = config_path("debugpy") { return Some(p); }
    if let Ok(p) = which("debugpy-adapter") { return Some(p); }
    if let Ok(p) = which("python3") {
        // Could be `python3 -m debugpy --listen ...`. Need to handle the multi-arg case.
    }
    None
}
```

### Step 3 — Routing

`crates/daemon/src/adapters/mod.rs`:

```rust
pub fn make_adapter(kind: AdapterKind, config: &Config) -> Result<Box<dyn DebugAdapter>> {
    match kind {
        AdapterKind::CodeLldb => Ok(Box::new(CodeLldbAdapter::new(config)?)),
        AdapterKind::DebugPy => Ok(Box::new(DebugPyAdapter::new(config)?)),
        AdapterKind::Fake => Ok(Box::new(FakeAdapter::new())),
        kind => Err(LazydapError::UnsupportedAdapter(kind)),
    }
}
```

### Step 4 — Auto-detect from filetype

`lazydap launch foo.py` — auto-pick `debugpy`. `foo.cpp` → codelldb. `foo.go` → delve (post-v0.1).

```rust
pub fn detect_adapter_for(program: &Path) -> Option<AdapterKind> {
    match program.extension().and_then(|e| e.to_str()) {
        Some("py") => Some(AdapterKind::DebugPy),
        Some("c" | "cpp" | "cc" | "cxx" | "rs") => Some(AdapterKind::CodeLldb),
        // ELF binary detection? Maybe. Probably overkill.
        _ => None,
    }
}
```

### Step 5 — Python example

`examples/py-hello/main.py`:

```python
def main():
    x = 5
    print("hello")
    y = x * 2
    print(f"y={y}")
    return 0

if __name__ == "__main__":
    main()
```

### Step 6 — Test

```bash
lazydap launch examples/py-hello/main.py --stop-on-entry --format json
lazydap break examples/py-hello/main.py:4
lazydap continue --wait --format json
lazydap eval "x" --format json    # 5
lazydap eval "y" --format json    # not yet defined → error
lazydap step --wait --format json
lazydap eval "y" --format json    # 10
lazydap disconnect
```

## Success criteria

- Python program debugged end-to-end via lazydap.
- TUI works for Python (source pane, stack, scopes, watches).
- Adapter auto-detection from filetype.
- Capability differences (codelldb has `supportsRestartFrame`, debugpy may not) handled gracefully — UI shows "(not supported)" rather than crashing.

## Files

- `crates/adapter-debugpy/` (new)
- `crates/config/src/discovery.rs` — extend with debugpy
- `crates/daemon/src/adapters/mod.rs` — wire DebugPyAdapter
- `examples/py-hello/main.py` (new)

## Verify

Manual: Python program debug via TUI and CLI. Both work identically (same key bindings, same response shapes).

## Depends on

- [`M15-config-file`](M15-config-file.md) — config/discovery patterns established.

## Notes

- **debugpy quirks** (per [`/docs/blueprint/03-adapters.md`](../../blueprint/03-adapters.md)):
  - Path mappings (`pathMappings`) for source resolution between local and remote (Docker). Out of scope unless you need it.
  - Subprocess debugging — out of scope until multi-session support.
- **You'll find hardcoded codelldb assumptions.** Probably in launch arg construction. Refactor `LaunchArgs` into per-adapter builders.
- **Capability divergence is real.** debugpy may not support `restartFrame`. The TUI should query capabilities and disable the relevant key binding.
- **Path conventions.** debugpy expects forward slashes everywhere; codelldb is fine with platform paths. Normalise in adapter code.
- **After M18, the project exits structured-milestone mode.** Future work tracked as issues / addenda.

---

## Completed — 2026-07-31

Python is debugged end to end through the same commands C is, and the adapter
seam D029 deferred is now a trait (D052).

### Deviations from the plan above

The plan was written before anyone had run debugpy. Four things it specifies
turned out to be wrong, and the code follows what the adapter actually does:

1. **No `crates/adapter-debugpy/`.** Both adapters are modules inside
   `crates/daemon/src/adapter/`. Separate crates would spread `lazydap_dap`
   across more manifests to buy something the existing module boundary and its
   `grep` already enforce. See D052.
2. **Not `debugpy-adapter`, and not TCP.** The plan preferred the
   `debugpy-adapter` binary "(simpler, more like codelldb)". It is a shim that
   commonly lands off `PATH`, and debugpy speaks **stdio**. The adapter is
   started as `python3 -m debugpy.adapter`, and the transport grew a stdio
   construction path (D053). Discovery therefore resolves an *interpreter*, and
   verifies it can import the module.
3. **No `LaunchArgs` refactor into per-adapter builders was needed** beyond a
   second typed struct: `PythonLaunchArgs` next to codelldb's `LaunchArgs`. The
   trait returns `serde_json::Value`, so each adapter keeps its arguments typed
   where they are written.
4. **`restartFrame` capability divergence never arose.** debugpy advertises
   *more* than codelldb, not less. The real divergence is `hitBreakpointIds`,
   which debugpy omits — quirk 5.

### Found by running it, not by reading it

Two bugs the gates could not have caught, both fixed here:

- **`continue` on an already-running program killed the session** under
  debugpy — it never answers such a request, the acknowledgement timeout fired,
  and that is read as a wedged adapter. Reached by `launch` without
  `--stop-on-entry` followed by `continue --wait`, which is what `launches run`
  does for most `launch.json` configurations. D055.
- **The debugpy presence check passed for `/bin/echo`**, because exiting zero
  proves nothing. It now requires the interpreter to print a sentinel back.

Two behaviours were left alone deliberately and are asserted so they cannot
drift: an uncaught Python exception is not a pause (D054's closing note), and
`step` on a running program can still reach an adapter timeout (D055).

### Where things are

- Trait, `Spawn`, discovery: `crates/daemon/src/adapter/mod.rs`
- Shared handshake: `crates/daemon/src/adapter/handshake.rs`
- The two adapters: `adapter/codelldb.rs`, `adapter/debugpy.rs`
- Transport (stdio + TCP + reverse requests): `crates/dap/src/transport.rs`
- Example: `examples/py-hello/main.py`; fixtures: `examples/py-fixtures/`
- Tests: `crates/daemon/tests/wait_debugpy.rs` (8, serialised like the codelldb suite)
- Quirks: [`docs/reference/debugpy-quirks.md`](../../reference/debugpy-quirks.md)

### Follow-ups discovered

- **`hit_breakpoint_ids` is empty under debugpy.** An agent that branches on
  which breakpoint was hit has to fall back to the frame. Synthesising ids by
  matching source and line was not done: it would be a guess on any line
  carrying two breakpoints, and a guess is worse than an honest empty list.
- **Exception filters.** `setExceptionBreakpoints` is never sent, for any
  adapter. Deciding whether an uncaught exception should pause is a product
  decision affecting both adapters; it wants its own milestone.
- **`step` while running** has the same unanswered-request shape as the
  `continue` bug and no sensible fallback. Worth a fast local rejection rather
  than a ten-second adapter timeout, on both adapters.
