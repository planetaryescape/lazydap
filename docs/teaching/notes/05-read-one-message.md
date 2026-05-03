---
chapter: 5
session_id: M1-1
title: Read one message
sessions_run:
  - date: 2026-05-03
    learner: bhekani
    duration_minutes: 90
    notes_below: true
---

# Teaching notes — Chapter 05: Read one message

## Concept anchor

**The one new concept**: parsing a length-prefixed-with-headers protocol off an async byte stream — `read_line` for terminator-known headers, `read_exact` for size-known body, `BufReader` to amortise syscalls, all on the *same* BufReader instance because the BufReader's internal buffer holds bytes that the underlying stream no longer has access to.

Subsumed under that one concept: the `lines()` method's `fn lines(self)` signature consumes the receiver, which Rust's ownership system enforces at compile time as the "same instance, both reads" rule. This is the core teaching moment of the chapter.

The chapter also runs a side-quest on TCP vs Unix sockets and on HTTP/1.1 framing identity, but neither is the *primary* concept — they're framing material that anchors the parser pattern in things the learner already knows.

## Common wrong predictions

| Predict question | Common wrong answer | Why learners go there | How the chapter currently calibrates |
|---|---|---|---|
| "How would you parse a Content-Length-framed message in Node?" | "Read everything as a string, split on `\r\n\r\n`, parse." | They have a *buffer-oriented* model from `await fetch(...).text()` where bytes are already in memory. Streaming feels theoretical until they try to write the read-everything loop. | The `<details>` block reframes: in a stream you don't have "the whole string"; you have to know when to stop reading; the header is exactly what tells you that. Two-phase parse emerges from that constraint. |
| "Why is naive byte-by-byte reading a bad idea?" | "Honestly, I don't know — making the kernel work too hard?" | Right intuition without vocabulary. Web devs mostly haven't thought about syscalls; the cost of `read()` is hidden behind libuv / asyncio. | Surface "syscall" + the mode-switch cost (~hundreds of ns to a few µs each). Use the warehouse-vs-truck mental picture for `BufReader`. |
| "Phase 1 vs Phase 2 read primitives — match them" | (typically correct) | The phrasing "terminator known" / "count known" makes the pairing obvious if the learner has been paying attention to the framing setup. | Confirm and move on. This predict is mostly a checkpoint that the prior framing landed. |
| "What does `read_line` actually return?" | "The line content as a string." | Reasonable expectation from `for line of file:` patterns in Python / Node's `readline`. Rust's `read_line` is uglier: it appends to a passed-in `&mut String` and returns the byte count as `usize`. | The mistake surfaces as a runtime bug (`let line = n.to_string()` — converting the byte count to a string and treating it as the line). The chapter pre-empts this by stating the API shape explicitly *before* they write the loop. |

## What surprised the learner

- **The `lines()` move-out as a compiler error, not a runtime bug**. The learner didn't anticipate that the borrow checker would catch the "same BufReader instance" rule. Their reaction: "I hit the exact footgun you flagged." This is a clean instance of compiler-as-co-teacher landing — the abstract rule from the start of the session became concrete via a real compile error 30 minutes later.
- **Drop runs on `?` early-return, not just on panic**. From M0-1 carry-over teach-back. The learner's mental model was "Drop is a panic safety net"; the sharpening to "Drop runs on every stack-unwind path including `?`-propagation" was a small but load-bearing correction.
- **Reads are byte-oriented, not just timing-dependent**. From the M0-1 teach-back. Learner said "if codelldb hadn't written line 2 yet by the time we read." Sharpened to "even if both lines were already written, reads return whatever bytes are in the buffer at that instant — could be half line 1, line 1, line 1 + half line 2, etc." This is the foundational fact of stream-based I/O and it's worth re-anchoring even if it was covered in the prior chapter.

## Sticky points (concepts that needed a second pass)

- **Why the empty-line check exists, and what `read_line` puts in the buffer**. The learner's first pass at the loop had `if buf == "\r\n\r\n"` as the break condition — but `read_line` reads *one* line including its terminator, so an empty line is just `"\r\n"` in the buffer, not `"\r\n\r\n"`. The chapter calibrates by spelling out the trim-then-check pattern explicitly.
- **`buf.clear()` placement**. First pass had it at the bottom of the loop, which technically works but is fragile (any new return path leaves stale content). The chapter places it at the top of each iteration as the defensive default.
- **`Option<usize>` vs `usize = 0` for content_length**. Subtle pedagogically — they both compile, both run; the difference is what happens when the header is missing. The chapter argues for `Option` as a forcing function: the `ok_or_else` at the end converts "header missing" into an explicit error rather than a silent zero-byte read.

## Refinement ideas

- [ ] Consider adding a quick raw `cargo run` snippet at the top of the "If you got something different" section showing the *exact* output to expect when the matcher hangs. The session learner said "it just hangs" but the actual symptom was specific (no output past the `cargo run` line). Concrete failure descriptions help solo readers debug. — apply on next chapter touch.
- [ ] The HTTP/1.1 side-quest could be moved earlier — it landed as a side-quest in the live session because the learner asked for it, but for solo readers the framing identity of HTTP/DAP is *the* anchor and it works better as the second concept slice. Currently it's framed as "side quest" which de-emphasises it. — wait for one more session's data point before moving.
- [ ] The `tokio::spawn` drain-stderr pattern is mentioned but not explained — could use a small "why we spawn instead of dropping" call-out. The chapter has it ("without this, codelldb's stderr pipe fills") but it's a one-liner. M0-1 chapter covered it more thoroughly; consider linking back rather than re-explaining. — apply on next session.
- [ ] The "verify before publishing" framework was a meta-discovery in this session — captured as teaching skill rule 16. Worth a brief inline mention in this chapter ("if your matcher doesn't match the live tool's output, that's environment drift; document it before assuming the chapter is wrong"). Currently implicit in the version-drift footnote. — apply on next chapter touch.

## Notes for future sessions on this chapter

- **Open with the partial-read recap from chapter 04.** It's the cliffhanger that motivates this chapter; without it, the framing problem can feel academic.
- **The compiler walkthrough is the centerpiece.** Don't preempt the `lines()` move-out by warning the learner away from `lines()`. Let them try it. Read the error together. The error itself is the teaching content.
- **Session ran 90 min in the live test** with full teach-back and skill-update propagation. Tighter than the 120-min budget, but the artifact verification + meta-discovery added load late. If the meta-discovery doesn't recur, future runs will likely be 75 min.
- **Anchor the drain-stderr pattern more aggressively in M0**, so this chapter can build on it instead of re-deriving. Already in the chapter 04 narrative; chapter 05 just references it.
- **For learners who already know Rust I/O reasonably well**, consider compressing concepts 1–2 (the three tools + TCP vs Unix sockets) into a single 5-min predict-and-go section, freeing time for the parsing loop. The compiler conversation needs the full time.

## Did the artifact land?

**Session 2026-05-03 (bhekani)**: yes. End-state: `cargo run --example m1_read_one_message` printed a real `initialize` response with `"success": true` and the full capability body. Clippy clean. No zombie codelldb process. Verified end-to-end.

The path to landing was: 5 phases, learner wrote phase 4, hit the `lines()` consume error, fixed it, hit a runtime bug (`n.to_string()`), fixed it, hit the version-drift hang, fixed the matcher across both example *and* milestone file, finally landed. Three different diagnostic modes used — compiler, runtime predict, direct-tool-run. Worth pocketing as a teaching pattern: when stuck, find the right diagnostic layer for the bug class.

## Reuse log

(Empty — first run.)

## Cross-cutting observation

The session uncovered that the M01 milestone task file and the chapter 04 drainage assumption both encoded a `"Listening on port N"` matcher that didn't match the live codelldb. M0-1 didn't bite because we never tried to *parse* the line; M1-1 was the first session that did. The fix went into:

- The example code (verified end-to-end)
- The milestone file (M01-read-one-message.md — matcher updated)
- This chapter (uses the version-tolerant matcher with footnote pointing at issues/0002)
- The teaching skill (rule 16 added: "verify before publishing")
- The bookgen skill (verify-before-publishing section added)
- The user's global CLAUDE.md (VERIFY BEFORE TEACHING framework)

This is the project's first instance of a session uncovering a meta-issue and propagating the fix four layers up the stack. Worth naming as a positive pattern: when a session reveals a teaching-content bug, fix it everywhere it lives, not just where it bit you.

## Smoke test status (rule 17)

**One smoke test added** at the bottom of `crates/daemon/examples/m1_read_one_message.rs` under `#[cfg(test)] mod tests`. Test name: `reads_a_content_length_framed_dap_message`. Run with `cargo test --workspace --all-targets`.

Design choices, picked to satisfy rule 17's no-load-multiplier constraint:

- **Real loopback `TcpListener` for fixture delivery**, not `tokio::io::duplex` or generics. Reason: the public function `read_one_message(&mut TcpStream)` keeps its concrete signature, no `<R: AsyncRead + Unpin>` introduced. A learner reading the example file sees no test-only refactoring.
- **Test asserts on the chapter's promise, not on internals.** From the chapter intro: "you'll see a response with `success: true` and a body of capability flags." The test asserts `value["type"] == "response"`, `value["command"] == "initialize"`, `value["success"] == true`. A learner could rewrite the parser using a state machine, a regex, or a hand-rolled byte loop — the test still passes. Implementation Swap Test: ✅.
- **One test, one behaviour.** The happy path is the chapter's central promise. Edge cases (missing Content-Length, EOF mid-headers, malformed JSON) wait for the dedicated TDD-1 session. A second test now would teach habits we haven't earned the right to teach yet.

CI updated (`.github/workflows/ci.yml`): `cargo test --workspace` → `cargo test --workspace --all-targets`. Examples are now exercised in CI.

When TDD-1 lands, the test in this chapter becomes a teaching artifact: "you've been seeing this test pass since chapter 05; today we explain how `#[tokio::test]` works, why we used `TcpListener::bind("127.0.0.1:0")` instead of mocking, and what the Implementation Swap Test means for the design choices we already made." Strong setup for the dedicated session.
