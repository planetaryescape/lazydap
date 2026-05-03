# 0002 — codelldb is silent without `RUST_LOG=debug` (version drift)

**Status:** open
**Discovered:** 2026-05-02 (session M0-1)
**Affects:** [`docs/implementation/tasks/M00-hello-adapter.md`](../implementation/tasks/M00-hello-adapter.md), and any future milestone that reads codelldb's startup output
**Priority:** medium (the M0 milestone doc gives wrong expected output; learners hit a hang)

## Summary

The M0 milestone doc claims:

> codelldb requires `--port 0` (TCP, not stdio). On port 0 it picks a free port and prints "Listening on port N" to **stderr**.

This **was true** for older codelldb versions but is **no longer true** as of codelldb v1.10+ (verified with v1.12.2 and lldb v20.1.4-codelldb shipped in Mason as of 2026-05-01).

In the current version, codelldb opens its TCP listener but writes **nothing** to stdout or stderr unless the `RUST_LOG` env var is set to `debug` or higher. Without it, a process spawning codelldb to read from its stderr will hang indefinitely.

## Verification

```bash
# Silent version (what M00 expected to print, doesn't):
~/.local/bin/codelldb --port 0 > /tmp/cl_stdout 2> /tmp/cl_stderr &
sleep 2
wc -c /tmp/cl_stdout /tmp/cl_stderr
# Both: 0 bytes

# Verbose version (what we now use):
RUST_LOG=debug ~/.local/bin/codelldb --port 0 > /tmp/cl_stdout 2> /tmp/cl_stderr &
sleep 2
cat /tmp/cl_stderr
# Output:
#   [INFO  codelldb] Loaded "/Users/.../liblldb.dylib", ...
#   [DEBUG codelldb] Listening on 127.0.0.1:NNNNN
```

The "Listening on port N" message moved from default-behaviour to `tracing`/`env_logger`-gated `DEBUG`-level. Background: codelldb adopted the tokio/`tracing` ecosystem somewhere around v1.9.

## Fix

Two updates needed:

1. **`docs/implementation/tasks/M00-hello-adapter.md`**: the example code snippet should add `.env("RUST_LOG", "debug")` to the Command builder. Update the "Expected output" section to reflect what's actually printed (the load + listening lines, both DEBUG-level). Update the "Run it 3 times: port number changes each time" success criterion. That criterion is now harder to verify because of the partial-read problem (Chapter 04 covers this) and isn't useful as a milestone gate.

2. **(future) `crates/adapter-codelldb/`**: when M0+ produces a real codelldb adapter implementation, the spawn config must set `RUST_LOG=debug` so the daemon can read the listening port. Comment in the adapter code referencing this issue.

## Why this matters beyond M0

The pattern, "library writes nothing without `RUST_LOG=debug`", generalises. Any Rust binary that uses `tracing`/`env_logger` will gate output behind `RUST_LOG`. Codifying this for codelldb in the adapter implementation prevents the same hang in future milestones.

## Cross-references

- [`docs/reference/codelldb-quirks.md`](../reference/codelldb-quirks.md): captures version drift and the `RUST_LOG` requirement
- [`docs/book/04-hello-adapter.md`](../book/04-hello-adapter.md): chapter teaches the `RUST_LOG=debug` env var as part of the spawn config
- Issue [0001](0001-codelldb-symlink-install-broken.md): related codelldb install issue (different root cause)

## Resolution criteria

- [ ] M00-hello-adapter.md's example code includes `.env("RUST_LOG", "debug")`
- [ ] M00-hello-adapter.md's "Expected output" matches current codelldb behaviour
- [ ] M00-hello-adapter.md's success criteria don't depend on output that codelldb no longer emits
- [ ] Future codelldb adapter implementation sets `RUST_LOG` for spawned children
