# Rust pain-anchor starter table

The pains Rust solves, indexed by which prior-language a learner felt them in. Use as the seed for `<output-repo>/docs/teaching/<lang>-anchor-table.md` when scaffolding a Rust book; extend with the learner's specific stated pains.

The lazydap project's full version lives at `~/code/planetaryescape/lazydap/docs/teaching/rust-anchor-table.md`. This is the portable subset.

## Per-feature pain table

| Rust feature | Pain it solves | Where you've felt it (typical) |
|---|---|---|
| `String` / `&str` | C's `char*` is just a pointer; no length stored, no UTF-8 guarantee, hope for `\0`. Rust adds bounded length, UTF-8 invariant, owned-vs-borrowed split. | C |
| Ownership + `Drop` | C's `malloc`/`free` pairing burden — every allocation needs a matching free, leak if you forget, double-free if you do it twice. Rust auto-`Drop`s deterministically when ownership ends. | C, C++ |
| Borrow checker | C's use-after-free, dangling pointers, iterator invalidation. Rust catches them at compile time. | C, C++, Go (data races) |
| Lifetimes | C lets you return a pointer to a stack variable; the resulting use-after-free is undefined behaviour. Rust's lifetime annotations make the compiler refuse to compile that. | C, C++ |
| `Result<T, E>` + `?` | C's "return -1, check `errno` separately." The function's return type doesn't tell you what can fail. JS's `throw` lets errors fly through any function invisibly. Rust's `Result` makes errors part of the type, `?` makes propagation a single character. | C, JS, Java |
| `Option<T>` | C's NULL pointer dereference; Java's `NullPointerException`; JS's "undefined is not a function". Rust's `Option` makes "may not exist" part of the type. Cannot use a `T` until the None case is handled. | C, JS, Java, Python |
| `Box<T>` | C's ambiguity about whether a pointer points to the stack or the heap. `Box<T>` is explicitly heap-allocated with single ownership and auto-cleanup. | C, C++ |
| `Vec<T>` | C's manually-grown arrays with realloc + bookkeeping. `Vec<T>` does this. | C |
| `match` (exhaustive) | C's `switch` — easy to forget a case; fall-through bugs. Java / TS switches similar (less so but still). Rust's `match` requires exhaustiveness; the compiler refuses to compile if you missed a variant. | C, Java, TS |
| Modules + `pub` | C's header-file dance (`.h` declares, `.c` defines, hope nothing diverges). Rust modules are visibility-controlled by `pub`; one source of truth per item. | C, C++ |
| Cargo | C's "what build system, what package manager, where do dependencies come from, what version" hellscape. JS's `node_modules` story is a *different* hellscape (works, but hostile to traceability). | C, C++, JS |
| Traits | C's lack of polymorphism beyond function pointers. TS's `interface` (similar, but no orphan rule, no blanket impls). Java/C# inheritance (different model, more rigid). Rust's traits give clean polymorphism with compile-time or runtime dispatch. | C, TS, Java |
| `Send` / `Sync` | C's data races and "I assumed this was thread-safe but actually...". Java's synchronized-keyword discipline. Go's "share by communicating" but with detection only at runtime. Rust marker traits make thread safety a compile-time invariant. | C, Java, Go |
| Async / `Future` | Node's callback-hell-then-promise-spaghetti journey; JS's "is this async or sync?" ambiguity. Rust's async makes the `Future` an explicit type that you `await`, with executor of your choice. | JS, Python (less so) |
| `unsafe` | C's "everything is unsafe by default — hope you knew what you were doing". Rust marks the unsafe boundaries explicitly so the safe code can be trusted. | C, C++ |
| Macros (declarative + procedural) | C's preprocessor is text-substitution. Rust's macros are syntactic — they operate on token trees. JS doesn't have macros (you just generate JS source files). | C |

## Anti-anchors (rare features without obvious pain anchor)

These exist in Rust but don't map cleanly to a pain in any common prior language. Spend extra time, not less. Strategy: *create the pain experience first* (write the broken-in-C version, watch it crash), then introduce the Rust feature as the fix.

- **Lifetimes** in their full subtype-relation form. The C pain (use-after-free of stack returns) is real but the learner may not have hit it yet if they've only worked in GC'd languages.
- **`Pin<T>` / async self-references**. Comes up when implementing futures by hand. Defer until the learner has a concrete need.
- **HRTB (Higher-Ranked Trait Bounds)**. Rare; defer.
- **Variance and the variance-checker**. Almost never relevant; defer indefinitely.

## Voice for the chapter prose

When invoking a row from this table in a book chapter, the framing is:

> **Pain anchor:** in C, [pain]. In JS, [other-language pain if applicable]. Rust handles this by [feature] which makes [the bad thing] either impossible or impossible-to-forget.

Lead with the pain, follow with the solution. *Don't* lead with "here's a Rust feature" and follow with "and by the way it solves a problem you've probably felt." The order matters.
