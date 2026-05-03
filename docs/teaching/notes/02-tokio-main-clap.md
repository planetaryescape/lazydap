---
chapter: 2
session_id: WS-2
title: Async main and clap
sessions_run:
  - date: 2026-05-01
    learner: Bhekani
    duration_minutes: 75
    notes_below: true
---

# Teaching notes — Chapter 02: Async main and clap

## Concept anchor

The chapter teaches **Rust attribute and derive macros as compile-time source rewriters**, explicitly contrasted with TS/Python decorators (which run at runtime). The unifying mental model: Rust attribute macros are like Babel plugins / TypeScript transformers, not higher-order functions.

`#[tokio::main]` and `#[derive(Parser)]` are the concrete vehicles; the chapter uses them to demonstrate the abstraction. The async semantics under `#[tokio::main]` are deliberately deferred. The learner gets the mental model of the macro, not what `Future::poll` actually does. Don't expand into deep async territory. Defer to a later session.

## Common wrong predictions

| Predict question | Common wrong answer | Why learners go there | How the chapter currently calibrates |
|---|---|---|---|
| "How do you think `#[tokio::main]` works?" | "It wraps `main` with another function that gives it async ability — like a TS or Python decorator wrapping a function with extra behaviour." | TS/Python decorators *are* runtime higher-order functions, and the syntactic similarity (annotation above a function) makes the analogy compelling. The model is half-right (yes, wrapping; yes, async-enabling) but conflates compile-time source rewrite with runtime function wrapping. | First `<details>` block flags the partial truth, then introduces "Babel plugin / TypeScript transformer" as the better hook. |
| "Will `async fn main` (no `#[tokio::main]`) compile?" | "No, because there's nothing to drive the future — it would just sit there." | Correct prediction! The learner reaches the right answer but often via the wrong reasoning ("the future would be silently dropped at runtime"). The actual mechanism is a *compile-time syntactic refusal*, not a runtime drop. | The chapter walks through E0752 and clarifies the compile-time-rule-vs-runtime-effect distinction. |
| "What sources will clap pull from to build `--help`?" | Two: field names and per-field doc comments. | Reasonable — those are the most visible inputs. But there are eight: doc comment above struct, `#[command(name=...)]`, field name, field type, field doc comment, `#[arg(default_value=...)]`, auto-added `--help`, auto-added `--version`. | The chapter makes this an explicit prediction with the "eight sources" reveal, designed to make the synthesis power of derive macros visible. |
| "What does the `#[tokio::main]` macro generate?" | "It adds a runtime initialisation line before the body" — i.e., partial expansion only. | Most learners predict the *runtime setup* part but not the **removal of `async` from `fn main`**. The macro turns the function from async to sync; that's the load-bearing transformation. | The chapter shows the approximate expansion explicitly — sync `fn main` wrapping `runtime.block_on(async { ... })`. |

## What surprised the learner

- **`async fn main` is rejected by the *compiler*, not just at runtime.** Many learners, including this one, assume "the future would be dropped" implies "runtime drops a future silently". Actually it's a hard syntactic rule (E0752) that fires before any code runs. The chapter makes this explicit; future learners should still get the same surprise from the predict pause.
- **The version `0.1.0` flowing from `[workspace.package]` (chapter 01) all the way through to `--version` runtime output (chapter 02).** Made the cumulative narrative concrete in a way that just *describing* it wouldn't have. **High-yield demonstration**, keep it. Consider adding a similar "trace the value end to end" exercise in any chapter where workspace-level config influences runtime behaviour.
- **Eight sources for `--help`**, more than they predicted. This was a useful "look how much one derive macro is doing" demonstration. Reuse the pattern for `Serialize` / `Deserialize` / `Debug` in later chapters when those derive macros land.
- **The constraint that derive macros can ONLY add `impl` blocks** (not modify the original) was new information for the learner. Worth keeping in the chapter explicitly. It's a crucial safety property when deriving on third-party types via `newtype` patterns later.

## Sticky points (concepts that needed a second pass)

- **The teach-back conflated "compile error happens because the future would be dropped" with "compile error happens because of E0752 (syntactic rule)."** Mechanism vs reason for the rule. The chapter calls out the distinction in the after-teach-back recap, but the first-pass model wobbled. Future runs: pre-empt this by stating "the compile rule exists *because* of the runtime issue, but the rule itself is syntactic."
- The phrase "TypeScript transformer" landed well, but only because the learner had used Babel before. For a learner whose web background is more recent (Vite-only, no Babel exposure), the reframe might miss. Consider an alternative: "Rust macros are more like a step in your build pipeline that rewrites .rs files into different .rs files, then the compiler reads the rewritten ones."

## Refinement ideas

- [ ] Add a brief mention of `cargo expand` as the way to *see* the post-macro source, with installation instructions. Currently mentioned only at the very end. Lift earlier; readers benefit from knowing the tool exists when the chapter first claims "the macro rewrites the source." Apply on next teach.
- [ ] The "Try it yourself" `--shout` exercise is good but the solution gives away the answer too readily. Consider adding one intermediate "your turn", e.g., "predict what type `shout: bool` will appear as in `--help`", before writing the code. Wait for 1+ data point.
- [ ] Consider lifting the "eight sources" exercise into a reusable demonstration template. Applies to any future derive macro the chapter sequence introduces. Mark for promotion to the pedagogy frameworks doc if the pattern repeats well in M2 (serde) and M5 (more clap).
- [ ] Don't expand async semantics. Multiple times mid-session it's tempting to dig into "what is a Future, really?". Defer it. Add a sidebar to the chapter explicitly saying "if you're curious about Future, that's a deliberately deferred topic; chapter [TBD] does it justice."

## Notes for future sessions on this chapter

- **The failing-first-then-fix demonstration** (write `async fn main` → hit E0752 → add `#[tokio::main]` → works) is the highest-yield move in the chapter. Empirical proof of compile-time rewrite. Reuse the *pattern* for any future compile-time machinery (lifetimes, type inference quirks, generic bounds, traits-not-in-scope). Don't skip this exercise even if the learner thinks they already know the result.
- **Mxr anchor moment**: when adding `tokio` to `[workspace.dependencies]` and inheriting it in `crates/daemon/Cargo.toml`, point at how mxr does the same thing. Reinforces chapter 01's inheritance lesson under a new dependency context.
- **The user reported feeling under-stimulated rather than fatigued at end of WS-1+WS-2 stacked.** The conceptual lift is present (decorators-vs-macros is real new content) but the mechanics dominate the time. Future sessions can move faster through the mechanical bits (writing the `[[bin]]` table, etc.) and dwell longer on the macro-as-source-rewrite reframe.
- **Use the Babel/TS-transformer reframe only if the learner has used Babel.** Otherwise: "compile-time source rewrite that runs before the compiler reads the file." That phrasing works for any learner.

## Did the artifact land?

Session 2026-05-01 (Bhekani): Yes. `cargo run -p lazydap-daemon -- --message hi --count 3` printed `hi` three times. The `--version` reveal also landed (workspace inheritance reaches runtime).

**Framing caveat**: per the WS-3 post-session addendum, the artifact was produced but *not framed as one*. The session closed on "we exercised macros" rather than "look what you can run now." Future runs: close with the demo command run live. Don't make the learner discover their own progress.

## Reuse log

(Empty — first run only. Update on next teach.)
