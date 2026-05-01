# How debuggers actually work

A reference for anyone working on lazydap (yourself, an agent, a contributor) who needs to know what's happening below DAP. lazydap is one layer of a many-layer cake; understanding the layers below makes lazydap's design choices comprehensible.

## The full layer cake

```
┌──────────────────────────────────────────────────────┐
│  YOU (or your TUI, IDE, agent)                       │
└────────────────────┬─────────────────────────────────┘
                     │ "set a breakpoint at line 42"
                     ▼
┌──────────────────────────────────────────────────────┐
│  Frontend: lazydap / VS Code / nvim-dap              │
└────────────────────┬─────────────────────────────────┘
                     │ DAP — JSON messages, just communication
                     ▼
┌──────────────────────────────────────────────────────┐
│  Adapter: codelldb / debugpy / dlv-dap / lldb-dap    │
│  ← speaks DAP on one end, native debugger on other   │
└────────────────────┬─────────────────────────────────┘
                     │ Library calls (C++ API, Python API, etc.)
                     ▼
┌──────────────────────────────────────────────────────┐
│  Real debugger: LLDB · GDB · sys.settrace · V8       │
│  ← THIS is what knows how to debug                   │
└────────────────────┬─────────────────────────────────┘
                     │ OS debugging API
                     ▼
┌──────────────────────────────────────────────────────┐
│  ptrace (Linux) · Mach exceptions (macOS) · Win32    │
│  ← syscalls into the kernel                          │
└────────────────────┬─────────────────────────────────┘
                     │ kernel handles process control,
                     │ memory access, signal delivery
                     ▼
┌──────────────────────────────────────────────────────┐
│  Your debuggee process — paused, inspected, resumed  │
└──────────────────────────────────────────────────────┘
```

DAP lives in one band. The actual machinery lives several layers below.

## Layer-by-layer

### Frontend (lazydap, VS Code, nvim-dap)

Translates user intent into DAP messages. Doesn't know what a register is. Doesn't know what `ptrace` is. Doesn't know what DWARF is. **Pure UX layer.**

### DAP — the protocol

Just JSON. `{"command": "setBreakpoints", "arguments": {"source": ..., "breakpoints": [{"line": 42}]}}`. **No code is executing on the debuggee at this layer.** It's a wire format between the frontend and the adapter, framed by `Content-Length: N\r\n\r\n` headers (same shape as LSP).

DAP is *purely communication*. It doesn't debug anything. It tells the adapter what the user wants and carries the adapter's response back.

### The adapter (codelldb, debugpy, etc.)

A *bridge process*. Speaks DAP on its stdin/stdout. Internally uses a real debugger library to do the work.

- **codelldb** embeds LLDB (the C++ library)
- **debugpy** uses Python's `sys.settrace` hooks
- **js-debug** talks to V8's Inspector Protocol
- **dlv-dap** embeds delve, which uses ptrace
- **lldb-dap** is a thin DAP wrapper around LLDB

The adapter's job: receive `setBreakpoints { line: 42 }`, translate to "LLDB, set a breakpoint at line 42 of main.c," return the result as DAP JSON. The adapter is where DAP meets the actual debugger.

### The real debugger (LLDB, GDB, runtime hooks)

**This is where debugging actually happens.** LLDB knows about DWARF debug symbols (which let it map "line 42 of main.c" to a memory address `0x4011a8`). It knows how to insert breakpoints, read registers, walk the stack, look up variables in the current scope.

Native debuggers (LLDB, GDB) use OS-level APIs. Managed runtime debuggers (debugpy, V8 inspector) use runtime-internal hooks instead.

### OS debugging API (ptrace, Mach exceptions, Windows Debug API)

Native debuggers use these to actually inspect another process. On Linux, `ptrace` is the syscall — it lets process A:

- Attach to process B (`PTRACE_ATTACH`)
- Pause B's execution
- Read B's CPU registers (`PTRACE_GETREGS`)
- Read B's memory (`PTRACE_PEEKDATA`, or modern `process_vm_readv`)
- Write to B's memory (`PTRACE_POKEDATA`) — this is how breakpoints actually get installed
- Resume B (`PTRACE_CONT`), single-step (`PTRACE_SINGLESTEP`)
- Receive notifications when B hits a signal

On macOS the equivalent is **Mach exception ports**. On Windows it's the **Windows Debug API** (`DebugActiveProcess`, `WaitForDebugEvent`, etc.). Same capabilities, different API surface.

### Kernel

Manages everything: stops the debuggee process when a breakpoint fires, delivers signals to the debugger, mediates memory reads/writes between the two processes' address spaces.

## Concrete example: how a breakpoint *actually* works

This is the bit most explanations skip and it's worth seeing once.

You set a breakpoint at line 42, which corresponds to address `0x4011a8` (the first byte of an instruction).

1. **lazydap** sends DAP `setBreakpoints { line: 42 }` to **codelldb**.
2. **codelldb** asks LLDB: "set a breakpoint at line 42 of main.c."
3. **LLDB** consults the DWARF debug symbols in the binary. DWARF says: "line 42 → address `0x4011a8`."
4. **LLDB** uses `ptrace(PTRACE_PEEKDATA, pid, 0x4011a8, ...)` to read the byte at that address. Suppose it's `0x55` (the first byte of `push rbp`).
5. **LLDB** uses `ptrace(PTRACE_POKEDATA, pid, 0x4011a8, 0xCC)` to overwrite that byte with `0xCC`. **`0xCC` is the x86 instruction `INT3` — a 1-byte trap.**
6. **LLDB** remembers: "address `0x4011a8` originally held `0x55`."
7. **codelldb** tells lazydap "breakpoint set, verified: true." lazydap shows a `●` in the gutter.

Now the debuggee runs. When the CPU's instruction pointer reaches `0x4011a8`, it executes `INT3`. The CPU raises a software interrupt. The kernel converts that into a `SIGTRAP` signal aimed at the debuggee.

8. The kernel pauses the debuggee and notifies LLDB (which had `PTRACE_ATTACH`'d, so it gets these notifications).
9. LLDB knows: "we hit our own breakpoint at `0x4011a8`."
10. LLDB tells codelldb. codelldb sends DAP `stopped { reason: "breakpoint", threadId: 1 }` to lazydap. lazydap shows `→` at line 42.

You inspect: `lazydap eval "x"`.

11. lazydap → DAP `evaluate { expression: "x", frameId: ... }` → codelldb.
12. codelldb asks LLDB: "what's `x` in this frame?"
13. LLDB looks up `x` in DWARF: "x is on the stack at offset `-0x4` from the base pointer."
14. LLDB reads the current value of `rbp` via `ptrace(PTRACE_GETREGS, ...)` — say it's `0x7ffeefbff5c0`.
15. LLDB computes `x`'s address: `0x7ffeefbff5c0 - 0x4 = 0x7ffeefbff5bc`.
16. LLDB reads 4 bytes there via `ptrace(PTRACE_PEEKDATA, pid, 0x7ffeefbff5bc, ...)` — say `0x00000005`.
17. LLDB tells codelldb "x = 5". codelldb → DAP → lazydap. You see `5`.

You press continue.

18. lazydap → DAP `continue` → codelldb → LLDB.
19. LLDB has to be careful here. The `INT3` it inserted is still at `0x4011a8`. To run line 42 properly:
    - Restore the original byte: `ptrace(PTRACE_POKEDATA, pid, 0x4011a8, 0x55)`.
    - Single-step the CPU one instruction: `ptrace(PTRACE_SINGLESTEP, pid, ...)`. The original `push rbp` executes.
    - Re-insert the `INT3` so we'll catch the breakpoint on the next call: `ptrace(PTRACE_POKEDATA, pid, 0x4011a8, 0xCC)`.
    - Now resume normally: `ptrace(PTRACE_CONT, pid, ...)`.

That's a breakpoint. Memory inspection. Register access. All happens via `ptrace` syscalls into the kernel.

## Native vs managed runtimes

The above is how it works for **native code** (C, C++, Rust, Go). The runtime is just CPU instructions; the debugger uses ptrace + DWARF to inspect raw memory.

**Managed runtimes** have built-in debugging hooks. The debugger doesn't need ptrace because the runtime cooperates:

- **Python**: debugpy uses `sys.settrace` — a built-in hook that fires Python callbacks on every line execution. The Python interpreter pauses itself when the debugger says so.
- **JavaScript (V8)**: js-debug talks to V8's Inspector Protocol — V8 exposes a WebSocket that emits debug events. Same interface Chrome DevTools uses.
- **JVM**: uses JDWP (Java Debug Wire Protocol) — JVM exposes a TCP port for debugger commands.
- **.NET**: uses ICorDebug (CLR debugger interface) or modern equivalents.

The DAP adapter for each managed language wraps the runtime-specific mechanism in DAP messages. Same protocol on top, very different machinery underneath. Adapters that wrap managed runtimes typically do *not* use ptrace.

## How this maps to lazydap's architecture

Per [`/docs/blueprint/01-architecture.md`](../blueprint/01-architecture.md), lazydap's stack is:

```
lazydap protocol  ← lazydap clients ↔ daemon
       ↓
DAP               ← daemon ↔ adapter
       ↓
ptrace / Mach     ← adapter ↔ debuggee  (or runtime hooks for managed)
```

Three protocols. Three failure surfaces:

- **lazydap protocol failures** → `tracing` logs in the daemon, IPC messages.
- **DAP protocol failures** → adapter's stderr, DAP transcript via `LAZYDAP_LOG_DAP=1`.
- **Native debug API failures** → adapter's logs; rare, usually means symbols missing or permissions wrong.

## Why "lazydap is just a wrapper on DAP" misses the point

lazydap is a wrapper on DAP. DAP is a wrapper on LLDB's API. LLDB is a wrapper on ptrace. ptrace is a wrapper on the kernel's process management. **Every useful tool is a wrapper on the next layer down.** The interesting question is what each particular wrapper lets you do that the layer below couldn't.

(See [`/docs/articles/yes-its-a-wrapper.md`](../articles/yes-its-a-wrapper.md).)

## What this means for hacking on lazydap

- You almost never touch ptrace directly. The adapter does.
- You almost never write DAP messages by hand. `crates/dap` types them.
- You write lazydap protocol handlers — the daemon's own surface.
- When something is broken, isolate the layer first: lazydap protocol bug? DAP message wrong? Adapter quirk? OS API issue? They look similar from above; the diagnostic tools differ.

## Further reading

- **DAP spec**: [microsoft.github.io/debug-adapter-protocol](https://microsoft.github.io/debug-adapter-protocol/)
- **DWARF**: [DWARF Debugging Standard](https://dwarfstd.org/)
- **ptrace**: `man 2 ptrace` (Linux), or [Eli Bendersky's "How debuggers work"](https://eli.thegreenplace.net/2011/01/23/how-debuggers-work-part-1) — the classic 3-part series
- **Mach exceptions**: [Apple's Mach docs](https://developer.apple.com/documentation/kernel/mach), darker corners of os/internals
- **Windows Debug API**: [Microsoft docs](https://learn.microsoft.com/en-us/windows/win32/debug/debugging-functions)
- **Tim Misiak's "Writing a Debugger from Scratch"**: [timdbg.com](https://www.timdbg.com/posts/writing-a-debugger-from-scratch-part-1/) — Rust, builds a real debugger from ptrace upward
