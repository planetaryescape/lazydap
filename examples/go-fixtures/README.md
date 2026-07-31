# go-fixtures — debuggees for the delve `--wait` tests

The Go half of [`c-fixtures`](../c-fixtures/README.md) and
[`py-fixtures`](../py-fixtures/README.md): small programs, each shaped to reach
exactly one of the outcomes `--wait` has to handle. They exist for the same
reason the others do — a timeout has to come from a program that really does not
stop — and they exist *separately* because M22's question is whether those
outcomes are reported identically for a third adapter. Asserting that against the
same programs would only prove the assertions match themselves.

`crates/daemon/tests/wait_delve.rs` runs them with the `dlv` it found on `PATH`.

| File | Reaches |
|---|---|
| `exits.go` | `state: exited`, `exit_code: 0` |
| `crashes.go` | an unrecovered panic — which **pauses** under delve, where the Python twin exits |
| `spins.go` | `state: timeout`: never stops on its own |
| `chatty.go` | a lot of `captured_output`, in order |

The line numbers matter: the tests set breakpoints by line. Each file says which
of its lines is load-bearing, in a comment on that line.

## Two things that are not true of the other fixture directories

**Nothing is built by hand, but something is built.** delve's `mode: "debug"`
compiles the source as part of the launch, so there is no build step here — but
unlike Python, the process that ends up running is *not* the file named below. It
is a binary delve compiled, which lazydap points at a temporary path so a leaked
one is findable and does not land in the repository. See
[`docs/reference/delve-quirks.md`](../../docs/reference/delve-quirks.md), quirk 4.

**There is a `go.mod`, and every file is `package main`.** Several `main`
functions in one directory means `go build ./...` fails, and that is fine:
nothing builds the directory. delve is given one file, `go build` is given one
file, and each is compiled alone. The module exists because building outside one
fails.
