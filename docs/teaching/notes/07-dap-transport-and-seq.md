---
chapter: 7
session_id: M2-2
title: DAP transport and atomic seq
sessions_run:
  - date: 2026-05-04
    learner: bhekani
    duration_minutes: 120
    notes_below: true
---

# Teaching notes — Chapter 07: DAP transport and atomic seq

## Concept anchor

**The one new concept**: the *transport pattern* — a struct that owns the I/O state (child process, persistent stream, sequence counter) and exposes one generic method that handles every protocol primitive of a wire-protocol-aware client.

Subsumed under that, in service of the pattern:
1. Generic methods with trait bounds (`<T: Serialize, R: DeserializeOwned>`) — extending yesterday's generic *types* to generic *methods*
2. `AtomicI64` for the seq counter — interior mutability through `&self` plus hardware-level race-freedom
3. `thiserror::Error` for ergonomic typed errors with `?`-propagation that composes via auto-generated `From` impls
4. Full-duplex demultiplexing — a `loop` that routes responses to the caller and events to log-and-skip (M3 will route them properly)

Heavier than chapter 06 in raw mechanic count. The cognitive-load discipline relies on framing 1, 3, and (partly) 4 as *applications* of things the learner already knows: generic types from yesterday, derive macros from yesterday, `match` on enum tags from earlier sessions. The genuinely-new bit is atomics, and the chapter explicitly defers memory-ordering semantics (Acquire/Release/Relaxed) to a future moment when they're provably needed.

## Common wrong predictions

| Predict question | Common wrong answer | Why learners go there | How the chapter currently calibrates |
|---|---|---|---|
| "What does `#[from]` invoke that makes `?` compile?" | "The Display trait, which is what we use to display the error." | This learner conflated `Display` and `From` *twice* across two sessions (chapter 06 and again chapter 07). Both are derive-attribute-generated traits; both relate to errors. The wrong intuition: "errors get displayed when they propagate." | The chapter calibrates with an explicit two-row table distinguishing `#[error("...")]` → `Display` (printing) from `#[from]` → `From` (`?`-propagation), plus a step-by-step walkthrough of `?`'s desugaring. This is a recurring confusion worth pre-empting verbally even if the table says it. |
| "What does `fetch_add` return?" | "The new value (after the +1)" | The natural English reading of `i = i + 1; return i` is "give me the new value." `fetch_add` follows the C `i++` post-increment convention (return previous, then add). | The chapter pairs the answer with the "claim a slot" mental model: each call reserves the *current* counter value for itself, then advances for the next caller. Initial value 1 → first call gets 1 (that's its seq), counter is now 2. |
| "Why AtomicI64 instead of plain i64?" | "I don't know" (honest); or "Rust requires it for async methods"; or "Atomic is faster" | All three are wrong but for different reasons. The honest "I don't know" is the right answer signal — atomics are a load-bearing concept that isn't in most senior TS/JS dev's tacit knowledge because JS's single-threaded model dodges the data-race problem entirely. | The chapter explains the C data-race problem from first principles (read-add-write is three instructions, two threads can interleave, an increment can be lost), then introduces atomic as "happens as one indivisible CPU instruction," then explains why this lets us mutate through `&self` (the safety boundary moves from the borrow checker to the hardware). The chapter explicitly steps DOWN a level when the first explanation feels too abstract. |
| "Why a loop in `request`?" | "To handle multiple concurrent requests with different seq IDs" | HTTP/2-style request multiplexing is a plausible model — many requests in flight, sort responses by ID. But DapTransport sends one request at a time. | The chapter clarifies the *real* reason: full-duplex event push. The adapter sends events at any time on the same stream as responses; the loop demuxes events from responses. The seq-matching is the *defensive* check for correlation, not the reason for the loop. Worth pre-empting because the wrong intuition (multiplexing) is the natural extension of "request-response" thinking. |

## What surprised the learner

- **The codelldb-quiet-without-RUST_LOG hang.** Learner ran the example and got nothing — no tracing output, no panic, just hang. Diagnosed by comparing m1 (works) vs m2 (hangs). Difference: m1's spawn helper sets `.env("RUST_LOG", "debug")` on the child Command; the new transport didn't. codelldb's `Listening on...` line is at debug level; without RUST_LOG=debug, the line never gets emitted, and our spawn loop blocks forever waiting on `next_line().await`. **Same class as M1-1's version-drift hang** — depending on adapter logging behaviour to discover its port. Worth flagging as a chapter teaching moment because the diagnostic path (m1-vs-m2 diff) generalises.

- **Tracing initialised but produced zero output.** When the binary hangs *before* any `tracing::debug!` fires, you see no tracing — but learners can mistake this for "tracing is broken." The diagnostic move: if you initialised tracing AND see no output, you're hung BEFORE any traced operation, not in spite of one. That points the diagnosis at the spawn step.

- **Q3 (loop) landed cleanly.** No surprise — the full-duplex framing landed once explained, and the bonus "would HTTP/1.1 need the loop?" was answered correctly without prompting. Senior TS dev internalises protocol distinctions quickly once named.

- **The AtomicI64 step-down request.** Learner explicitly asked for a simpler explanation when the first one used vocabulary they didn't have ("interior mutability via atomic CAS", "memory ordering"). Excellent learner self-awareness — they paused the firehose. The C-anchor step-down (data race example, three-instruction read-add-write) landed properly. **Worth keeping in the chapter as the *primary* explanation, not as a fallback** — the C data race story is more concrete than "interior mutability through &self" and arrives at the same destination.

## Sticky points (concepts that needed a second pass)

- **`From` vs `Display` confusion (recurring).** Learner conflated these in both M2-1 (the error-handling discussion preview) and M2-2 (the `?` walkthrough). Twice in two sessions. The correction lands when explained; the *intuition* doesn't stick. Possibilities for chapter revision:
  1. Add a leading "two completely independent traits" framing BEFORE either is introduced
  2. Use side-by-side examples — show `?`-propagation triggering `From` (no Display call) AND `eprintln!("{err}")` triggering `Display` (no From call) in two separate code blocks
  3. The chapter currently has the table; consider promoting it earlier in the section

  Apply approach #2 on next session's data point — if a third learner conflates them, it's a structural chapter issue not a learner-specific gap.

- **`DeserializeOwned` vs `Deserialize`.** Mentioned as a sidebar in the chapter; learner accepted it as deferred-load. No friction in this session, but worth tracking — when lifetimes land formally (likely M5 or later), this is the moment to revisit and explain *why* DeserializeOwned exists.

- **`get_mut()` on BufReader<TcpStream>`.** Came up briefly when explaining how the transport writes to the stream. Learner accepted "BufReader buffers reads only; writes go through `.get_mut()` to reach the underlying TcpStream" without questioning. Could deserve a footnote next refresh, but didn't bite this session.

- **`Ordering::SeqCst` deferred load.** Learner asked nothing about it. The chapter's "trust me on it for now" framing held. Acquire/Release/Relaxed is a real future-session topic; not worth introducing here.

## Refinement ideas

- [ ] **Lead the From-vs-Display section with a more aggressive disambiguation.** Currently the table is mid-section; given the two-session repeat confusion, try opening the thiserror section with: "These are two completely independent traits doing two completely independent jobs. They share an enum but they don't talk to each other." Then introduce them. — apply on next session's data point.

- [ ] **Move the C data race example to the *primary* atomics explanation, not the step-down fallback.** It's more concrete than "interior mutability through &self." The current chapter does this in the `<details>` block; promote to the main flow. — apply on next chapter touch.

- [ ] **Add a one-liner about JS's single-threaded model dodging the problem.** Senior JS/TS devs may genuinely not have hit data races before because the runtime serialises everything. A footnote like "if you've never written multithreaded code, this might be your first encounter with data races — that's normal; JS's event loop hides the problem entirely" would land softly. — apply on next chapter touch.

- [ ] **The smoke-test discussion is currently a "note" section.** Consider promoting to its own section since the chapter's lack of a unit test is a deliberate pedagogical choice (rule 17 boundary) that future sessions will revisit. The reasoning is worth being prominent. — wait for one more session to gauge if learners ask about tests.

- [ ] **The `tokio::spawn` for stderr drain pattern is mentioned but not explained.** This was covered in chapters 04-05, so re-explanation here would be redundant. Consider linking back rather than restating. — apply on next chapter touch.

## Notes for future sessions on this chapter

- **Plan for ~120 minutes.** Heavier than chapter 06 (which was ~90). Pacing felt sustainable but no margin for unexpected detours. If a learner takes 90 min to land yesterday's chapter, plan for 120-150 here.

- **Open with the call-site diff as the motivation.** The chapter currently buries the "every command is now a one-line call" insight in the ladder check. Promoting it to the top — "today we replace ad-hoc per-call code with a reusable transport; here's what every future call site looks like" — would frame the work better.

- **The HttpClient analogy is load-bearing.** Don't skip the surface-the-model section even with a high-energy senior learner. The "where does this sit on the spectrum" framing (fetch ↔ DapTransport ↔ Stripe SDK) is what makes the design choice (one generic method, not one per command) feel deliberate rather than arbitrary.

- **Atomics need the C data-race example, not the &self framing.** First-pass explanation in this chapter used "interior mutability via &self" which assumes the learner has internalised that mutation normally requires `&mut`. Faster to lead with "two threads racing on a shared int" and arrive at &self at the end as the *consequence* of atomics, not the starting frame.

- **Watch for the From/Display conflation.** Every learner so far has tripped on this at least once. Pre-empt by explicitly framing them as "two independent traits, neither calls the other" before either is introduced.

- **The codelldb-quiet hang IS the chapter's compiler-conversation moment.** It's not technically a compile error, but it serves the same pedagogical role: the runtime says "I am stuck waiting for X" and the diagnostic dance teaches a generalisable principle (compare-vs-known-good, run-the-writer-directly, look for the layer the bug is at). Keep it.

## Did the artifact land?

**Session 2026-05-04 (bhekani)**: yes. End-state: `cargo run --example m2_initialize` printed real Capabilities from real codelldb, with the typed `DapTransport` doing the full request/response/demux dance. Workspace clippy clean. The trace with `RUST_LOG=dap=debug,lazydap_dap=debug` showed the protocol clearly: codelldb's startup, the connection acceptance, our typed request going out, the response coming back, and the typed deserialisation landing.

The path to landing: the learner wrote the example, hit the silent hang (codelldb-quiet-without-RUST_LOG), followed the diagnostic path (m1-vs-m2 diff), found the missing `.env("RUST_LOG", "debug")` on the Command builder, and the example ran clean on the second attempt. Pedagogically clean — the bug was *projecting* an invariant from m1 (which the chapter relies on) into the new transport, and the fix surfaces a generalisable diagnostic principle.

Atomics required a step-down to land. First-pass explanation used vocabulary the learner didn't have ("interior mutability", "memory ordering"). Learner explicitly asked for a simpler explanation; the C data-race step-down landed cleanly. Worth pocketing as a teaching pattern: when a senior learner says "go simpler," they're saving you from compounding load — honour it immediately.

## Reuse log

(Empty — first run.)

## Cross-cutting observation

This is the first session in lazydap that produces an *infrastructure* artifact rather than a *protocol-level* one. M0-M1 exercised "see the wire", M2-1 made the wire typed, M2-2 wraps the wire in reusable infrastructure. The pedagogical signal: from here on, future sessions are about adding *capabilities* (events, breakpoints, scopes) to a foundation that's now stable. The transport will not change again until M3 layers events on top.

Worth surfacing to the learner explicitly at session open or close: "you just shipped the foundation. From M3 on, every session adds a feature; nothing rewrites the transport." That framing makes the work feel cumulative rather than ladder-y — each future session builds *on* this, not *toward* something.

Also notable: this session's bug (the codelldb-quiet hang) is the **third** instance of "an invariant from a prior chapter quietly didn't transfer to the new code." The pattern:
- M1-1: milestone task file's port matcher assumed `"Listening on port N"`; codelldb v20 emits `"Listening on HOST:PORT"`. Hang in lesson.
- M2-2 (today): m1's `.env("RUST_LOG", "debug")` got dropped when porting spawn into the transport struct. Hang in lesson.
- (Future watch:) the BufReader-instance discipline from M1-1 — same instance for both reads — could quietly drift if someone splits read paths. Not yet bitten but worth tracking.

This is consistent with the verify-before-publishing discipline that landed as teaching skill rule 16 from M1-1. **Each chapter's invariants need to be explicit assumptions of the next chapter's code, not implicit "remember from before"** — when they're implicit, version drift in the chapter that introduced them silently breaks the chapter that depends on them. Promote to a teaching skill refinement after one more data point.

## Smoke test status (rule 17)

**No unit-test smoke test by design.** Documented in the chapter narrative. The chapter's promise is irreducibly integration-flavoured ("real codelldb, real DAP wire, real typed Capabilities back"); a unit test would require either a generic-stream refactor or a test-only constructor, both of which add API surface that doesn't serve the chapter's promise.

Instead, the example file `crates/daemon/examples/m2_initialize.rs` is the smoke test. It builds in CI (`cargo build --workspace --all-targets`), so an API regression in the transport fails compilation. Behavioural verification requires running it with codelldb installed — same trade-off as chapter 04.

When chapter 08 (M3) introduces event streaming, the testable invariants change (event channel correctness can be unit-tested without codelldb), and the smoke-test posture should change with it. Track this as a future "we said no smoke test for chapter 07; here's what changes for chapter 08."

The three unit tests from chapter 06 (in `crates/dap/src/types.rs`) still serve as smoke tests for the *type definitions* and continue to run in CI. They don't cover the transport, by design — types and transport are separate concerns.
