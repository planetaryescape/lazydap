# Rust anchor table — pain-first teaching

For the lazydap teaching project. The user knows JS/TS deeply, Python well, and is currently learning C in parallel. When teaching a Rust concept, **anchor on the pain it solves in a language they've used**. The framing "you know how X is painful in C? Rust fixes that by Y" lands deeper than "in Rust, you have to do Y."

Most useful column right now: the **C pain** column. C is live for the user; pains are felt right now. Use them aggressively.

## Memory and ownership

| Rust concept | C pain it solves | JS/TS analog | Python analog | Where the analogy breaks |
|---|---|---|---|---|
| **Ownership + automatic `Drop`** | `malloc` requires matching `free`; forget = leak; do twice = double-free crash. Manual lifetime tracking everywhere. | Garbage collector hides this; "no memory bugs" is a partial truth (closures hold references, leaks happen) | GC same as JS | Rust runs `Drop` *deterministically* at scope end, not "eventually" like GC. Useful for files/sockets/locks too, not just memory. |
| **`Box<T>`** | `T*` from `malloc`. Is it stack or heap? Who owns it? Did caller pass me a borrowed pointer or transferred ownership? | All objects are heap-allocated by default; you don't think about it | Same as JS | `Box<T>` is *explicitly* heap-allocated with *exactly one* owner. Free is automatic at scope end. |
| **`Vec<T>`** | Manually-grown arrays: `realloc` when full, copy on growth, track length and capacity, free on done | `Array` (auto-resizes, GC) | `list` (auto-resizes, GC) | Vec is contiguous memory like C; it's not a linked list. Push is amortised O(1); reallocation copies all elements. |
| **`String` / `&str`** | `char*` is just a pointer to first byte; no length stored, no UTF-8 guarantee, hope for trailing `\0`. Buffer overruns galore. | JS strings are objects with `.length`; UTF-16 internally; immutable | Python `str` is unicode; opaque internals | `String` owns + tracks length + enforces UTF-8 invariant. `&str` is a borrowed view (pointer + length, no `\0`). Splitting safely on UTF-8 char boundaries is non-trivial — Rust forces you to think about it. |
| **Borrow checker (`&T` and `&mut T`)** | Use-after-free, dangling pointers, iterator invalidation, returning pointer to local. C lets you do all of these; bugs blow up in production | JS doesn't have aliasing rules; references are everywhere | Python similar to JS | Compile-time enforcement: at any moment, *either* one mutable reference *or* many immutable references, never both. The infamous "fighting the borrow checker" — that's the compiler protecting you from C-style bugs at compile time. |
| **Lifetimes (`'a`)** | Returning `T*` to a stack variable; passing pointer to data that will be freed; iterator outliving its container | (no analog — GC handles it) | Same | The compiler annotation that proves no dangling reference is possible. **The hard cliff for the user — no anchor in JS/Python; closest is "what would prevent C's use-after-free at compile time."** |
| **`Rc<T>` / `Arc<T>`** | Manual reference counting (you write the `retain` / `release`, it's hard to get right) | (you don't think about it; GC) | Python uses refcounting + GC under the hood | `Rc` is single-thread refcount (no atomic overhead); `Arc` is atomic refcount (threadsafe). Both auto-decrement and free at zero. Cycles still leak — use `Weak<T>`. |
| **Stack vs heap** | Implicit: `int x = 5;` → stack; `int* x = malloc(sizeof(int));` → heap. Bugs from confusing the two | Hidden — primitives often "boxed", everything-is-an-object pretence | Hidden | In Rust, you choose. Default values go on the stack; `Box`, `Vec`, `String` allocate on the heap. The compiler tracks; the language makes it visible. |

## Errors and absence

| Rust concept | C pain it solves | JS/TS analog | Python analog | Where the analogy breaks |
|---|---|---|---|---|
| **`Result<T, E>`** | "Return -1 and check `errno` — but what if -1 is a valid value? What if `errno` got clobbered by an unrelated call?" | `try` / `throw` / `catch`, but errors aren't in the type signature; functions silently throw | Same as JS — exceptions everywhere | `Result` makes errors part of the *type*. Compiler refuses to compile if you don't handle the error case. The `?` operator makes propagation a single character. No silent throws. |
| **`?` operator** | Manually checking every return code: `if (rc < 0) { goto cleanup; }` everywhere | `try { await f() } catch (e) { ... }` blocks; flat with `Promise.catch` | `try/except` blocks | `?` short-circuits on error, propagates up. One character vs C's multi-line goto-cleanup pattern. JS's try/catch unwinds invisibly; `?` is visible in the source. |
| **`Option<T>`** | NULL pointer dereferences. C lets you do `*ptr` without checking; segfault in prod | `T \| undefined`; `if (x)` checks; "undefined is not a function" runtime errors | `None`; `if x is None`; `AttributeError` | `Option<T>` makes "may not exist" part of the type. Compiler refuses to let you `unwrap()` into the value without acknowledging it might be None. No silent NPEs. |
| **`unwrap()` / `expect()`** | Implicit dereference of a pointer that could be NULL | `value!` non-null assertion in TS — explicitly opting into "trust me" | `assert x is not None` | Explicit "this should never be None; panic if it is" — at least the panic point is in your code, not at some weird call site. The discipline is to use `expect("why this can't be None")` so the message helps debug. |

## Type system

| Rust concept | C pain it solves | JS/TS analog | Python analog | Where the analogy breaks |
|---|---|---|---|---|
| **Sum types / tagged unions (`enum`)** | `union` in C is untagged — caller has to remember which field is valid. Tags are a separate variable; easy to get out of sync | TS discriminated unions (`type X = { kind: 'a', ... } \| { kind: 'b', ... }`) — same idea but at runtime not compile-time | Type unions via `Union[A, B]` or runtime checks | Rust's `enum` is a tagged union with the tag *always* in sync. `match` on it is exhaustive — compiler refuses to compile if you missed a variant. |
| **Pattern matching (`match`)** | `switch` with `break` everywhere; fall-through bugs; no destructuring | TS `switch` (no exhaustiveness check), or `if (x.kind === 'a') { ... }` | `match` (3.10+) — same idea, less common in idiomatic Python | `match` destructures + binds variables + is exhaustive. The compiler is your safety net. |
| **Traits** | C has function pointers and convention; no formal interface contracts. `qsort` takes a comparator function pointer with no compile-time guarantees | TS `interface` — closest analog | Python's protocols / `abc.ABC` | Traits add: orphan rule (you can only `impl` a trait for a type if you own one of them), coherence (no two implementations conflict), blanket impls (`impl Trait for T where T: OtherTrait`), no inheritance (composition only). Compile-time vs runtime dispatch is your choice (`impl Trait` vs `dyn Trait`). |
| **Generics** | C has macros (`#define`) — text substitution, no type checking, ugly errors | TS generics, similar shape | Python's typing.Generic | Rust generics are compile-time monomorphised — each instantiation gets its own copy. Zero runtime cost. Trait bounds (`<T: Display>`) replace duck-typing with compile-time checks. |
| **Type inference** | C requires every type to be spelled out | TS infers types where possible, especially in const initialisers | Python uses runtime types; type hints optional | Rust infers nearly everywhere except function signatures. The result: little type ceremony, but all type checking still happens. |
| **`u8` / `u32` / `usize` / `i64` etc.** | `int` is platform-dependent in size; `long` is platform-dependent. Endianness, overflow, all undefined behaviour territory | All numbers are doubles; integers don't really exist | `int` is arbitrary precision; less footgun-prone | Rust types are *exact*: `u8` is exactly 8 bits, `i64` is exactly 64 bits signed. `usize` is the platform's pointer size (32-bit on 32-bit systems, 64-bit on 64-bit). Overflow is checked in debug, wraps in release (configurable). |

## Concurrency

| Rust concept | C pain it solves | JS/TS analog | Python analog | Where the analogy breaks |
|---|---|---|---|---|
| **`Send` and `Sync` (auto-traits)** | "I assumed this was thread-safe but actually..." Data races. `pthread_mutex_lock` everywhere; forgetting one = race condition with no warning | Single-threaded by default; web workers communicate by message | `threading` exists but GIL makes it weird; data races possible with `multiprocessing` | Rust marker traits enforce thread safety at compile time. `Send` = "OK to move to another thread." `Sync` = "OK to share a reference across threads." Compiler refuses to spawn a thread closing over data that isn't `Send`. |
| **`Mutex<T>` and `RwLock<T>`** | `pthread_mutex_t` separate from the data it protects; easy to lock the wrong one or forget to lock | (rarely needed in single-threaded JS) | `threading.Lock`, but the lock and the data are still separate variables | Rust's `Mutex<T>` *contains* the data. You can't access the data without locking; the type system enforces it. The lock guard auto-releases on scope exit (RAII). |
| **`async fn` / `await`** | Callbacks, `select` / `poll`, manual state machines | Promises + async/await — closest analog | `asyncio` — closest analog | Rust async is *zero-cost* — compiles to state machines, no runtime overhead beyond the executor (Tokio, async-std). But it interacts with ownership (`Send` futures, `'static` bounds). **Defer deep async semantics until M11+.** Use `#[tokio::main]` and `await` without explanation for now. |

## Build and tooling

| Rust concept | C pain it solves | JS/TS analog | Python analog | Where the analogy breaks |
|---|---|---|---|---|
| **Cargo** | "What build system? Make? CMake? Autotools? Meson? How do dependencies work — apt? brew? building from source?" Hellscape | npm / yarn / pnpm — closest analog | pip / poetry / uv | Cargo is the *one* build system + package manager + test runner + doc generator. Universal across the ecosystem. Lockfile-based reproducibility. Just works. |
| **Modules (`mod`, `pub`)** | Header files (`.h` declares, `.c` defines). Hope they don't diverge. `#include` is text substitution. Include guards. Symbol pollution | ES modules with `import`/`export` | Python `import` | Rust modules are file-based: one source of truth per item. `pub` controls visibility (default private). No header dance. |
| **Workspaces (`Cargo.toml [workspace]`)** | Multiple `Makefile`s coordinated by hand or by a top-level Make | npm workspaces / pnpm workspaces | (no native equivalent; monorepos use external tooling) | Multiple crates under one umbrella, sharing dependencies and build cache. Native to Cargo from day 1. |
| **Documentation (`cargo doc`, `///` comments)** | Manpages, separate docs, often outdated. README is what you get | TSDoc / JSDoc, generated separately | Sphinx, separate config | `///` comments + `cargo doc --open` produces searchable HTML. Doctests (executable examples in docs) prevent doc rot. |
| **Tests (`#[test]`, `cargo test`)** | Whatever framework you cobbled together; CMake test runners; manual main functions | Jest / Mocha / Vitest, separate from build | pytest, separate runner | Built into the language. `#[test]` annotation, `cargo test` runs them. Doctests, integration tests, benchmarks — all standard. |

## Pattern of language design

Things to call out as "Rust takes the side of safety":

- **No null** — `Option<T>` makes absence explicit
- **No exceptions** — `Result<T, E>` makes errors explicit
- **No GC** — `Drop` makes resource release explicit and deterministic
- **No undefined behaviour** in safe Rust — `unsafe` blocks are explicit
- **Exhaustive matching** — compiler refuses incomplete handling
- **Move semantics by default** — no implicit copies, no aliasing surprises

The user has felt the pain of each of these in C (or will). Frame them as "Rust took the C pain and put the cure in the type system."

## Pattern of language design (the JS/Python perspective)

Things to call out as "Rust gives you back what dynamic languages took away":

- **Compile-time errors** — no "this works in dev, breaks in prod because of a typo"
- **Type-level documentation** — function signatures tell you everything, no docs needed
- **Refactoring confidence** — change a type, the compiler shows every place that needs updating
- **Performance** — predictable, no GC pauses, no boxing surprises

For someone who's done 8 years of "carefully not breaking the JS prod," Rust's compile-time guarantees feel like a relief, not a constraint. Frame it that way.

## How to use this table in sessions

When introducing a Rust concept:

1. Find it in the table.
2. Lead with the **C pain** column if the concept has one (most do — Rust is largely a C-pain-fixing language).
3. Cross-reference the JS/TS column for "and here's how it differs from what you're used to in TS."
4. Use the **Where the analogy breaks** column to flag the specific bit that the analogy doesn't capture — say it explicitly.

When you don't find the concept in the table (e.g., `impl` blocks, closures, the `let` keyword), use the standard analogy approach without the pain framing. Not every Rust feature exists to fix a pain — some are just how Rust expresses things.

## Maintenance

Add rows to this table as new concepts come up in sessions. The table grows with the project. When 3+ concepts cluster around a sub-topic that doesn't have its own row group yet, add a new section header.

## See also

- [`docs/teaching/sessions.md`](sessions.md) — per-session plan
- [`docs/teaching/README.md`](README.md) — what this directory is
- [`/AGENTS.md`](../../AGENTS.md) — teaching mode protocol
- Obsidian: `Teaching Senior Engineers.md` — the pedagogy synthesis
- Obsidian: `Lazydap Teaching Sessions.md` — session log
- The teaching skill: `~/.dotfiles/.agents/skills/teaching/SKILL.md`
