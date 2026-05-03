---
chapter: 4
session_id: M0-1
title: Hello, adapter
sessions_run:
  - date: 2026-05-02
    duration_minutes: 75
    notes_below: true
---

# Teaching notes — Chapter 04: Hello, adapter

## Concept anchor

The one new concept is **spawning external processes asynchronously with `tokio::process::Command`**. Everything else (the Drop-trait gloss, `Option::take`, the trait-import gotcha) lands in service of getting one external process spawned, read-from, and cleaned up safely. If a session expanded past that, covering full DAP framing or genuine Drop-trait implementation, the chapter scope was overrun and should be split.

## Common wrong predictions

| Predict question | Common wrong answer | Why learners go there | How the chapter currently calibrates |
|---|---|---|---|
| Where does spawning a child process come from, `std` or `tokio`? | `std` (and reading from `tokio`, stdio config from `std`) | The Node mental model is "child_process is just stdlib", so they assume `std::process` is the same thing | Concept 1 introduces the std-vs-tokio split with the I/O-vs-config rule of thumb and the table of mirror types. Reframes std as "sync-by-default, no forced runtime" |
| What does `Command::new(...)` return? | "A pipe" / "a child process" / "the spawned thing" | They think construction = spawn (Java/Python `subprocess.Popen()` precedent) | The "Critical correction" sidebar in Concept 2 names that `Command::new` returns a *config object* and the process appears at `.spawn()` |
| Why does `main` return `anyhow::Result<()>`? | "Because we have `Ok(())` inside" / "anyhow captures errors" (causal arrow reversed; they describe the consequence, not the prerequisite) | They reason from what's in the body to what's at the signature, instead of the other way around | The chapter's `<details>` reverses the arrow explicitly: "the return type *enables* `?`, the `Ok(())` is just the success-case payload" |
| Will running 3 times verify port-changes? | "Yes" / "It depends on stable codelldb output". Most don't predict the partial-read-misses-the-port-line outcome | The original milestone doc said the port message comes through; learners assume the doc is authoritative | The chapter explicitly de-promotes the "verify port changes" criterion and uses the partial-read miss as the live demonstration of Concept 7 |
| Would taking bytes count as mutating `child`? | "Maybe, taking bytes from it into the buffer". They conflate *reading bytes via stderr* with *mutating child* | The mutation-via-`.take()` is genuinely subtle; learners look for the obvious mutation (the read) rather than the field-extraction one | Concept 5 + the deliberate-`mut`-removal experiment in Compiler Conversation makes the propagation visible via E0596 |

## What surprised the learner

- **codelldb is silent by default in newer versions.** The "Listening on port N" message is gated behind `RUST_LOG=debug`. Without it, the program hangs on the read because there's nothing to read. Original milestone doc was wrong here; chapter now teaches `RUST_LOG=debug` as part of the spawn config.
- **codelldb's symlink-vs-wrapper-script gotcha.** The standard `ln -sf` install pattern that the project's own CONTRIBUTING.md recommended *did not work*. codelldb computes its `liblldb.dylib` location from `argv[0]`, and a symlink invocation produces a wrong relative path. Chapter and CONTRIBUTING.md now require a wrapper script. Captured fully in `docs/reference/codelldb-quirks.md`.
- **The partial-read outcome wasn't what was predicted** (both lines together). The read returned the INFO line but missed the DEBUG line. A clean demonstration of the partial-read gotcha; the chapter leans into this rather than papering over it.
- **`unused_imports` not silenced by `unused_must_use = "deny"`.** Different lints. Don't conflate them.

## Sticky points (concepts that needed a second pass)

- **`Command::env` adds vs Node's spawn `{ env }` replaces.** Brief gotcha but worth highlighting. Many readers will assume Node-style replacement.
- **`let _x = ...` vs `let _ = ...`** banked as a future footgun. Worth a tiny callout in the chapter (currently a parenthetical) but full treatment can wait until a later chapter that actually uses `let _ = ...` on a Drop-meaningful value.
- **The reason-for-`mut` propagation** was *easier* to land via the deliberate-error compiler conversation than through prose alone. Don't try to teach this rule abstractly first; skip to the experiment.

## Refinement ideas

- [ ] Add a `mxr` cross-reference if mxr does anything analogous to `tokio::process::Command` spawning. Could deepen the "your past code is the best teacher" anchor (operating rule 10). Defer until checking mxr's actual code.
- [ ] Consider a sidebar on `tracing` / `env_logger` and the `RUST_LOG` convention as a Rust-idiom lookup. Won't bloat the chapter; it's a 4-bullet aside. Apply when a learner asks "wait, what is `RUST_LOG`?"
- [ ] The "Common wrong predictions" data is thin (one learner). Don't lift any of these to *explicit* chapter explanations until 3+ learners independently make the same mistake. (Operating rule: data threshold for chapter revision = 3.)
- [ ] If learners consistently miss that `Command::new` is config-not-spawn, consider making *that* the predict question instead of "what crate does it come from?" The crate question is too easy; the lifecycle question hits a deeper bug.

## Notes for future sessions on this chapter

- **Open with the artifact promise from rule 13.** "By the end you'll have made first contact with a real debugger." The artifact is what holds the long-arc motivation.
- **Don't pre-install codelldb for the learner.** The install surprise (wrapper-script-not-symlink) is itself teaching content. If pre-installed correctly, the learner misses the dynamic-library-loading lesson.
- **Encourage paste-and-edit, not paste-and-run.** Learners who only paste the final example don't internalise the builder pattern. Push them to type the chained calls and predict each method's return type.
- **The deliberate-`mut`-removal experiment is gold.** It's the chapter's strongest concrete moment of compiler-as-co-teacher. Don't skip it because it feels redundant. The surprise is genuine.
- **codelldb version matters.** If the learner's codelldb is older than v1.10, `RUST_LOG=debug` may not be needed. Check version with `codelldb --help` before assuming the chapter's spawn block is correct.

## Did the artifact land?

**2026-05-02 session:** Yes. The example printed the codelldb INFO log line. Did NOT print the "Listening on port" line (partial-read miss), but the chapter explicitly recasts that as the lesson rather than the failure. Learner could run the example end-to-end on their own and verify the artifact.

## Reuse log

- 2026-05-02: original session that produced the chapter (lazydap project, primary learner).

(Future entries: when this chapter has been used to teach this concept *outside* the project's main session sequence, e.g., a refresher for a returning learner, note it here. Helps spot when the chapter has earned promotion to "stable" status vs needing more refinement.)

## Smoke test status (rule 17)

**No smoke test for this chapter.** Teaching skill rule 17 (added 2026-05-03) requires teacher-written smoke tests for each session's outcome. M0-1 is an exception: the chapter's promise — "spawn codelldb, read its first stderr chunk, kill cleanly" — is irreducibly integration-flavoured. Verifying it requires a real codelldb on the runner; mocking the spawn would either need a generic-over-`Command` refactor (visible load multiplier) or a different binary entirely (which tests our spawn pattern, not the chapter's lesson). Both would distort the example.

The chapter's verification stays manual: `cargo run --example m0_hello_adapter` is run by hand by the teacher each time the chapter is touched. Rule 16 (verify before publishing) covers it.

If the dedicated TDD-1 session reveals a way to test M0-1 cleanly without distortion, retrofit then. Until then, document the gap rather than write a test that's either fragile in CI or doesn't actually test the chapter's promise.
