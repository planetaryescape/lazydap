# 16 — Addendum

Post-blueprint amendments. Added as the project progresses and reality diverges from the original plan.

Format: `## A001 — short title`. Date. Status. What changed and why.

## A001 — DAP transport is per-adapter, not "stdio"

**Date:** 2026-07-30
**Status:** decided

### What changed

`01-architecture.md` describes the DAP layer as "Content-Length-framed JSON **over stdio**" and its
layer diagram draws `dap` below `protocol`/`store`/`config`. Reality: codelldb is TCP-only
(`--port 0`, port announced on stderr — see `docs/reference/codelldb-quirks.md` quirk 3), debugpy
speaks stdio, and the shipped `DapTransport` uses `TcpStream`. Transport setup is an adapter-owned
detail behind the `DebugAdapter` trait. The dependency **table** in `ARCHITECTURE.md` (dap depends
on nothing internal) is authoritative; the diagram is not.

### Why

Discovered while building M0–M2 against real codelldb. An agent designing the `DebugAdapter` trait
from the diagram would wire the wrong dependency direction and assume a universal stdio transport.

### What this affects

- File: `docs/blueprint/01-architecture.md` (diagram + Layer 2 prose left as-is; this entry is the diff)
- Code: `crates/dap/src/transport.rs` today; `DebugAdapter` trait at M5; adapter crates at M18

---

---

## A001 — (placeholder)

When you find that something in `00-overview.md` through `15-decision-log.md` doesn't match what you actually built, add an entry here. Don't quietly edit the original blueprint files — the history matters. The blueprint is the original plan; this addendum is the diff.

Template:

```
## A00N — title

**Date:** YYYY-MM-DD
**Status:** decided | proposed | superseded
**Supersedes:** (Dxxx in 15-decision-log if applicable)

### What changed

(One paragraph)

### Why

(Why we changed it. Concrete: what bit us, what we observed.)

### What this affects

- File: relevant blueprint doc(s)
- Code: relevant crate(s)
- Tests: relevant test(s)
```
