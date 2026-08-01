# c-fixtures — debuggees for the `--wait` tests

Small C programs, each one shaped to reach exactly one of the outcomes
`docs/blueprint/10-async-to-sync.md` requires `--wait` to handle. They exist
because those outcomes cannot be faked: a timeout has to come from a program
that really does not stop, and an `AdapterDied` has to come from an adapter
that really is gone.

`crates/daemon/tests/wait_codelldb.rs` compiles them on demand (with `cc`, or
`gcc`) into a temporary directory. Nothing here is built by `cargo`, and
nothing here is linked into lazydap.

| File | Reaches |
|---|---|
| `exits.c` | `state: exited`, `exit_code: 0` |
| `crashes.c` | a segfault — `paused` with an exception reason, or `terminated` |
| `spins.c` | `state: timeout`: never stops on its own |
| `chatty.c` | a lot of `captured_output`, in order |
| `threads.c` | several threads, for the coalescing window |
| `floods.c` | more output than a wait will carry, then more after a pause (D070) |
| `inspects.c` | a stop worth reading: a large array, and an unreadable pointer (D067, D068, D074) |

The line numbers matter: the tests set breakpoints by line. Each file says
which of its lines is load-bearing, in a comment on that line.
