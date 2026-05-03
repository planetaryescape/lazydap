---
chapter: 6
session_id: M2-1
title: Serde and typed protocols
sessions_run:
  - date: 2026-05-03
    learner: bhekani
    duration_minutes: 90
    notes_below: true
---

# Teaching notes — Chapter 06: Serde and typed protocols

## Concept anchor

**The one new concept**: serde derive macros eliminate the type-vs-validator drift that languages like TypeScript+Zod and Python+Pydantic each handle differently. By writing the struct *once* with `#[derive(Serialize)]` / `#[derive(Deserialize)]`, a procedural macro reads the type definition at compile time and emits the runtime parser/emitter as Rust source. The struct *is* the schema.

Subsumed under that one concept, all in service of the same idea:
- `#[serde(rename_all = "camelCase")]` for the bulk-rule case
- `#[serde(rename = "...")]` for per-field overrides
- `Option<T>` as the optionality exception to the "missing field = error" default
- Generic struct definitions `DapResponse<R>` for shape-varying-by-context wrappers, with the macro auto-emitting the `R: Deserialize` bound

The chapter also runs the call-site diff (chapter 05's `value["body"]["foo"].as_bool()` vs chapter 06's `resp.body.unwrap().foo`) as the closing artifact. That diff is the deliverable — proof that the bug class "silent miscompile from a typed JSON walk" is gone.

## Common wrong predictions

| Predict question | Common wrong answer | Why learners go there | How the chapter currently calibrates |
|---|---|---|---|
| "What does `as DapResponse` do at runtime in TS?" | (typically correct) "TS types are erased; `as` is a compile-time lie." | Senior TS devs have hit this enough to know. Junior or non-TS learners might think `as` does runtime validation. | The `<details>` block confirms the type-erasure framing and bridges to "what would Zod give you that `as` doesn't?" |
| "What happens when you `from_slice::<Capabilities>` against camelCase JSON with snake_case struct fields?" | Often "succeeds and ignores fields it can't match" or "fails because the struct has more fields than the JSON" | Buffer-oriented model: serde feels like it should be lenient. Real model is strict-by-default. The casing mismatch isn't on the radar at all — learner reasons about *number* of fields, not *names*. | The `<details>` block surfaces both policies (extras silently ignored, missings hard error) AND flags that the *bigger* issue is name mismatch — none of the names match anyway because of casing. |
| "DapResponse has no `rename_all`. What does that tell you about DAP's wire format for the wrapper?" | (typically correct after the `Capabilities` lesson lands) | Once they've internalised "`rename_all` translates" they correctly invert: "no `rename_all` = exact match = wire format is snake_case." | Confirms the inference and surfaces the broader pattern: DAP is *bilingual* about casing. Envelope = snake; bodies = camel. Each struct owns its own rule. |

## What surprised the learner

- **`#[derive(Default)]` and `#[serde(default)]` are independent things.** Multiple learners assume that deriving `Default` on the struct tells serde to use the default for missing fields. It doesn't — serde requires the explicit `#[serde(default)]` attribute (or `Option<T>` field type) to opt out of the missing-field error. This came up in passing during the calibration; worth pre-empting in the chapter on a future revision.

- **The `lines()` move-out from chapter 05 didn't appear here**, but the *general pattern* did — "the macro reads your type, generates code." Learner asked: "is `derive` doing what `cargo expand` showed for `#[tokio::main]`?" Yes — it's the same procedural-macro mechanism, just with a different target (Serialize/Deserialize instead of fn rewrite). Worth a one-line callback to chapter 02 on a future revision.

- **The compiler's "consider borrowing here" hint on `to_string`.** Learner reads the compiler error verbatim and applies the fix without prompting — first time the "compiler suggests, you accept" pattern landed without external scaffolding. Worth noting in the chapter as a "if you see help: consider X, do X."

- **JSON equality ≠ string equality.** This was the standout meta-moment of the session. Learner wrote `assert_eq!(produced_json, hand_written_json)` and the test failed despite the serialisation being correct. They had to be walked through "the LEFT side proves the test code's *production* is right; the diff is about the *assertion strategy*, which is a separate question." The chapter calibrates by leading the runtime-error walkthrough explicitly with "read the LEFT side first, you won."

## Sticky points (concepts that needed a second pass)

- **Serialise vs deserialise direction muddle.** First pass at the test had `let init_args: InitializeArgs = serde_json::to_string(...).expect(...);` — assigning the result of `to_string` (which is a `String`) to a variable typed `InitializeArgs`. The conceptual confusion: "serialise" was being treated as a round-trip rather than a one-way conversion. The compiler error (`expected InitializeArgs, found String`) does flag it directly, but the *meaning* of the error needed unpacking. The chapter calibrates by stating explicitly: "Serialisation is one-way: struct → JSON string. The result is a String, not a struct."

- **`Option<String>` field literals in struct construction.** First pass had `client_id: "1234"` (where struct field is `Option<String>`). Compile error is clear (`expected Option<String>, found &str`) but the fix involves two transformations: wrap in `Some(...)` AND convert `&str` to `String` via `.into()`. Multi-step fix from a single error. Chapter calibrates by including the explicit fix in the compiler-conversation section: `Some("lazydap".into())`.

- **One miscount in the prompt.** I told the learner *three* fields needed per-field overrides (`clientID`, `clientName`, `adapterID`) — but `clientName` is clean snake→camel (`client_name` → `clientName`), so only TWO need overrides. The learner caught my error by *not* overriding `clientName` and trusting `rename_all`. Worth keeping the table of wire-vs-Rust names as a deliberate "find the exception" exercise rather than fixing the count, because the exercise is exactly to identify which fields break the bulk rule. Chapter currently keeps the table accurate (two overrides, not three).

## Refinement ideas

- [ ] **Surface `#[serde(default)]` vs `#[derive(Default)]` distinction earlier.** Learner was uncertain whether `Default` on the struct affected serde's missing-field behaviour. Currently a parenthetical in the chapter; could be a short call-out before the optionality discussion. — apply on next session.
- [ ] **Connect to chapter 02 on procedural macros.** The "macro reads your type at compile time" framing assumes the learner remembers chapter 02's coverage from clap's side. A one-line "(remember `#[tokio::main]` from chapter 02 — same mechanism)" would help. — apply on next chapter touch.
- [ ] **Add a brief mention of `serde_json::Value::eq` for the JSON-equality footnote.** The chapter currently mentions "parse both sides into Value and compare" as the production-ready approach but doesn't show the syntax. One example block would close the loop. — wait for one more session's data point before adding (the substring approach was sufficient pedagogically; only worth adding `Value::eq` if a future learner gets confused about how to do production-grade JSON compare). 
- [ ] **The `path_format: bool` type bug.** Learner wrote this on first pass — `pathFormat` in DAP is a string. The wire-vs-Rust table in "Try it yourself" doesn't currently spell out types. Adding a third column (`Type`) might pre-empt the bug. But the bug is itself useful — surfaces "I should look up what DAP actually sends here." Trade-off; defer judgment until next session. — wait for another data point.

## Notes for future sessions on this chapter

- **Open with the call-site diff as a teaser, not a closer.** Currently the chapter ends on the diff. Showing it up front as "here's the bug class we're going to eliminate today — the runtime walk" might motivate the work better. Try this on the next live run.
- **Keep the failing-test-then-rename_all sequence.** This is the load-bearing pedagogical move of the chapter. Deleting the failure step and going straight to the right code skips the most important lesson — that the failure is what tells you *which annotation to reach for*. Don't shortcut.
- **Three rounds of gradual release worked clean.** I-do (Capabilities), we-do (DapResponse<R> with predicts), you-do (InitializeArgs from a table). Time budget held — round 3 took ~25 min including two iterations against the compiler. Future runs likely 20 min if the learner has seen `#[derive]` on something else first.
- **Watch for serialise/deserialise direction confusion in round 3.** It's the highest-frequency error-class and the only conceptual one (the others are mechanical). If a learner writes `let x: T = serde_json::to_string(...)`, redirect with "serialise = struct → string, one direction; you don't get a struct back." Don't just pile on the compile errors.
- **Don't introduce `#[serde(skip_serializing_if = "Option::is_none")]` unless asked.** It's a real wrinkle (None becomes `null` on the wire by default; skipping it is a separate annotation), but it's a load multiplier. DAP tolerates `null` for optional args in practice; defer until M2-2 or a learner explicitly asks.

## Did the artifact land?

**Session 2026-05-03 (bhekani)**: yes. End-state: `cargo test -p lazydap-dap` printed three green tests covering the bilingual deserialise (Capabilities), the generic wrapper (DapResponse<Capabilities>), and the per-field-override serialise (InitializeArgs). Workspace clippy clean. The call-site diff demonstrated cleanly — typed access on chapter 06 vs `value["..."].as_bool()` walks on chapter 05.

The path to landing: 3 rounds of gradual release, 2 compile-error iterations on round 3 (`&` borrow + `Option<String>` literals), 1 runtime-failure walk (JSON-equality vs string-equality). Three different diagnostic layers, each appropriate to its bug class — compile errors for type mismatches, runtime errors for value mismatches, IDE/protocol-spec lookup for the wire-format gotchas. Worth pocketing as a teaching pattern: this chapter naturally exercises three diagnostic layers in one session.

## Reuse log

(Empty — first run.)

## Cross-cutting observation

This session's call-site diff (chapter 05's `value["body"]["foo"].as_bool().unwrap_or(false)` vs chapter 06's `resp.body.unwrap().foo: bool`) is the most concrete demonstration yet of "Rust shifts runtime errors to compile time." The learner had previously *understood* the pitch but seeing it land on real code they wrote yesterday was the lightbulb. Worth keeping the side-by-side framing — it's the chapter's emotional payoff.

Also: the meta-question the learner asked mid-session ("did we choose wrong by using a test instead of an example for the diagnostic?") was a good moment to surface the diagnostic-layer-matching framework. Tests are good for isolating one variable (the casing rule); examples are good for end-to-end integration; compile errors are good for type-shape mismatches; runtime errors are good for value mismatches. Each tool for its own job. The framework was implicit in chapter 05's session note; explicit in this chapter's compiler-conversation section. Promote to a project-level reference doc when one more session adds a fourth data point.

## Smoke test status (rule 17)

**Three smoke tests** in `crates/dap/src/types.rs` under `#[cfg(test)] mod tests`. Each asserts on the chapter's outcome promise:

1. `deserialises_a_capabilities_body` — verifies `rename_all = "camelCase"` does its job on the body type.
2. `deserialises_a_full_initialize_response` — verifies the bilingual envelope-body pattern, the generic instantiation, the per-field rename for `type`, and `Option<T>` for optional `message`.
3. `serialises_a_initialize_args_body` — verifies struct-level `rename_all` + per-field `rename` overrides compose correctly on the outbound side. Asserts on the *substring* presence in the produced JSON, not character-equality.

Design choices:

- **Tests are pure** (no I/O, no codelldb, no TCP). Rationale: the chapter's promise is about the JSON↔Rust mapping, not about the wire transport. The wire transport is chapter 07's lesson. Keeping tests pure isolates the variable.
- **Tests assert on chapter promise, not internals.** A learner could rewrite `Capabilities` using a hand-written `Deserialize` impl, or a different rename strategy, or even split into multiple structs — the tests would still pass as long as the public contract (camelCase JSON in, populated struct out; struct in, expected wire-format names in the JSON) holds. Implementation Swap Test: ✅.
- **The serialise test uses substring-match assertions.** Justified in the chapter narrative. Production code would prefer `Value`-comparison; teaching code is more transparent with substrings.
- **No edge-case coverage** (unknown fields, malformed JSON, missing required fields). Those wait for TDD-1 (the dedicated meta-session before M5-2). One test per chapter promise; nothing more.

CI is already configured for `cargo test --workspace --all-targets` (from chapter 05). No CI change needed for chapter 06.
