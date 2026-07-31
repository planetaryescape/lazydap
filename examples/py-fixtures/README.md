# py-fixtures — debuggees for the debugpy `--wait` tests

The Python half of [`c-fixtures`](../c-fixtures/README.md): small programs, each
shaped to reach exactly one of the outcomes `--wait` has to handle. They exist
for the same reason the C ones do — a timeout has to come from a program that
really does not stop — and they exist *separately* because M18's question is
whether those outcomes are reported identically for a second adapter. Asserting
that against the same programs would only prove the assertions match themselves.

`crates/daemon/tests/wait_debugpy.rs` runs them with the interpreter it found.
Nothing here is compiled, which is the one real difference from `c-fixtures`:
there is no build step, no fixed output path, and none of the macOS
code-signing warm-up that makes the C fixtures insist on a stable path.

| File | Reaches |
|---|---|
| `exits.py` | `state: exited`, `exit_code: 0` |
| `crashes.py` | an uncaught exception — Python's version of the segfault case |
| `spins.py` | `state: timeout`: never stops on its own |
| `chatty.py` | a lot of `captured_output`, in order |

The line numbers matter: the tests set breakpoints by line. Each file says which
of its lines is load-bearing, in a comment on that line.
