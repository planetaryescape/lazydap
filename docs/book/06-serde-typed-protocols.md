---
chapter: 6
session_id: M2-1
title: Serde and typed protocols
phase: A
estimated_time_minutes: 90
artifact: A `crates/dap` crate with typed `Capabilities`, `DapResponse<R>`, and `InitializeArgs` structs that round-trip real DAP wire formats — replacing the `serde_json::Value` walk from chapter 05 with compile-time-checked field access
prerequisites:
  - Chapter 05 (read-one-message) — comfortable with the bytes-to-Value pipeline
  - You've used `serde_json::Value` and `value["body"]["foo"].as_bool()` once and felt the runtime-walk weight
  - Procedural macros at a hand-wave level (chapter 02 met them from clap's side; today we meet them from the user's side)
new_concepts:
  - `#[derive(Serialize)]` and `#[derive(Deserialize)]` — the macro generates parser/emitter code from the type definition
  - `#[serde(rename_all = "camelCase")]` — struct-level naming rule
  - `#[serde(rename = "...")]` — per-field override that composes with `rename_all`
  - `Option<T>` as the optionality exception to "missing JSON field = error"
  - Generic struct definitions (`DapResponse<R>`) for shape-varying-by-context wrappers
related_milestone: docs/implementation/tasks/M02-initialize-handshake.md
---

# Chapter 06 — Serde and typed protocols

> Session ID: `M2-1` · Phase A · ~90 min · [Underlying milestone](../implementation/tasks/M02-initialize-handshake.md)

## What you'll learn

How to make the JSON ↔ Rust boundary compile-time-safe. Today's tool: **serde derive macros**. Two struct-level annotations (`rename_all`), one per-field override (`rename`), one optionality marker (`Option<T>`), and one generic type parameter (`<R>`) cover ~95% of every typed-protocol Rust crate you'll ever write. The remaining 5% are edge cases the docs handle.

The deeper move: **the type definition becomes the single source of truth for both the compile-time shape and the runtime parser.** No drift, no mirrored Zod schema, no `JSON.parse(...) as MyType` lie.

## What you'll build

A new `crates/dap` crate with three typed structs — `Capabilities`, `DapResponse<R>`, `InitializeArgs` — that handle real DAP wire shapes both ways. Three small tests that round-trip them.

> By the end of this chapter, running:
>
> ```bash
> cargo test -p lazydap-dap
> ```
>
> will pass three tests:
>
> ```
> test types::tests::deserialises_a_capabilities_body ... ok
> test types::tests::deserialises_a_full_initialize_response ... ok
> test types::tests::serialises_a_initialize_args_body ... ok
> ```
>
> And the call site for reading a capability flag becomes:
>
> ```rust
> let resp: DapResponse<Capabilities> = serde_json::from_slice(&body)?;
> let supports = resp.body.expect("body").supports_configuration_done_request;
> //              ^^^^^^^^^^^^^^^^^^^^^^^^ compile-time-checked field access
> ```
>
> Compare to chapter 05's `value["body"]["supportsConfigurationDoneRequest"].as_bool().unwrap_or(false)`. Three runtime hash-map walks, no autocomplete, typo silently → false. **That's the bug class today's chapter eliminates.**

## Before you start

**Prior knowledge assumed:**

- You've used `serde_json::Value` and felt the weight — three runtime walks per access, typos pass through silently.
- TypeScript: you know that types are erased at runtime and `as MyType` is a compile-time annotation only. You've reached for Zod (or io-ts, or Yup) to add runtime validation.
- Python: you know `json.loads()` returns `dict | list | int | str | bool | None` and you've reached for Pydantic / `dataclass` + a validator to add runtime shape-checking.

**Setup state required:**

- `cargo run --example m1_read_one_message` succeeds (chapter 05 is shipped).
- `serde_json` already lives in `[workspace.dependencies]` (you added it in chapter 05).

If either fails, fix it before continuing.

---

## Surface your model first

> 🤔 **Q:** Forget Rust for a moment. In TypeScript:
>
> 1. What does `JSON.parse(jsonString) as DapResponse` actually *do* at runtime? What guarantees does that `as DapResponse` give you?
> 2. If you wanted real *runtime* checks (does the JSON actually have the shape `DapResponse` claims?), what library or pattern would you reach for? What's the cost vs the cast?
>
> Same question for Python if that's where your reflex is: how does `json.loads(s)` typing differ from a Pydantic model, and what does Pydantic *do* that plain `json.loads` doesn't?

<details>
<summary>Click after you've answered</summary>

The right answer (for TS): `JSON.parse` returns `any` (or `unknown` if you've configured it strictly). The `as DapResponse` annotation is **erased at runtime** — TypeScript types don't exist after compilation. So `as DapResponse` is purely a *promise to the compiler* that has no enforcement at runtime. If the JSON has the wrong shape, your code happily accesses fields that don't exist; you get `undefined` lookups and runtime crashes downstream.

To get real runtime checks, you reach for **Zod** (or io-ts, Yup, valibot, etc.). Zod gives you `z.object({ command: z.string(), success: z.boolean(), ... }).parse(jsonString)` — runtime schema validation that throws on mismatch.

The cost: **two sources of truth.** You have a TypeScript type (or interface) *and* a Zod schema, and you have to keep them in sync manually. You can use `z.infer<typeof schema>` to derive the TS type *from* the Zod schema, which collapses one direction — but you've now committed to defining your types via Zod's runtime DSL, not via plain TypeScript syntax.

Python: same pattern. `json.loads()` returns untyped Python primitives. Pydantic adds a class-based schema (`class DapResponse(BaseModel): command: str; success: bool`) that does runtime validation in the `BaseModel` constructor. The schema and the type are unified in Pydantic's case (the class definition is both), but Pydantic is a separate library you opt into.

</details>

---

## Concept 1 — Derive does what Zod does, but *from your type definition*

Serde collapses the two-source-of-truth problem.

You write the struct **once**, with `#[derive(Deserialize)]` (or `#[derive(Serialize)]`, or both) above it. A **procedural macro** runs at compile time, reads your type definition, and emits the runtime parser as Rust source code that the compiler then compiles into the binary alongside everything else.

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Capabilities {
    pub supports_configuration_done_request: bool,
    pub supports_function_breakpoints: bool,
    pub supports_conditional_breakpoints: bool,
}
```

That's it. After this lands you can write `serde_json::from_str::<Capabilities>(json)?` and you get a real `Capabilities` struct back, with three real `bool` fields. If the JSON doesn't match the shape, you get a runtime error with a precise message. **One source of truth — the type — becomes the compile-time shape AND the runtime parser.**

The mental model:

| | TypeScript + Zod | Python + Pydantic | Rust + serde |
|---|---|---|---|
| **Compile-time type** | TS interface | Type-hint annotations | Rust struct |
| **Runtime validator** | Separate Zod schema | Pydantic `BaseModel` (subclass) | Generated by macro from the struct |
| **Source of truth** | Two (drift risk) | One (subclass `BaseModel`) | One (the struct itself) |
| **Cost of validation** | Hand-write schema | Inherit from BaseModel | One `#[derive(...)]` line |

Pydantic is closer to serde than Zod is — both unify the type and the validator into one definition. The thing serde adds: it's just a derive on a plain Rust struct. No subclassing, no DSL, no separate schema language. The struct is plain Rust; the macro reads it.

> **Pain anchor:** in TS, `JSON.parse(s) as DapResponse` is a *lie* — the runtime has no idea what shape you claim. Zod adds runtime checks but at the cost of writing the schema twice (once for TS, once for Zod). Serde generates the runtime parser *from* the type. No drift surface.

Now let's see why naively writing the struct above doesn't work — and what one annotation fixes it.

---

## Concept 2 — `rename_all` for the bulk

We've got the simplest possible struct. DAP sends us this JSON for the body of an `initialize` response:

```json
{
  "supportsConfigurationDoneRequest": true,
  "supportsFunctionBreakpoints": true,
  "supportsConditionalBreakpoints": true
}
```

Note the casing mismatch: JSON is `camelCase` (DAP wire format, inherited from JS land); Rust struct fields are `snake_case` (idiom, also enforced by clippy).

> 🔮 **Predict:** Take the `Capabilities` struct from Concept 1, derive `Deserialize`, and call `serde_json::from_slice::<Capabilities>(&body)` against the camelCase JSON above. What happens?
>
> 1. Does it deserialize successfully and populate the `bool` fields with the JSON's `true` values?
> 2. Does it fail with a "missing field" error?
> 3. Does it succeed but leave all fields as their default `false`?
> 4. Something else?

<details>
<summary>Click after you've predicted</summary>

It fails with a missing-field error. The reason is more thorough than "the struct has a different number of fields." Two policies are in play:

- **Extra JSON fields** (in JSON, not in struct): silently ignored by default. (Strict mode is `#[serde(deny_unknown_fields)]` if you want extras to error.)
- **Missing JSON fields** (in struct, not in JSON): hard error by default.

But the *bigger* issue is that **none of the names match anyway**. JSON is `supportsConfigurationDoneRequest`; the struct field is `supports_configuration_done_request`. Serde compares names character-for-character. From serde's POV: all three struct fields are "missing from JSON" *and* all three JSON fields are "unknown to the struct." Two simultaneous mismatches.

The actual error you'd get:

```
Error("missing field `supports_configuration_done_request`", line: 5, column: 9)
```

Notice the error names the **Rust** field, not the JSON field. Serde reasons from the struct's POV: "I need to find `supports_configuration_done_request` in the JSON; not there." It doesn't know that a *similarly-named* `supportsConfigurationDoneRequest` exists in the payload — they're different strings to a character-equality comparison. Casing convention is a human convention; serde doesn't apply it for free.

</details>

So how do we tell serde the rule "every snake_case Rust field maps to camelCase JSON"? Three options:

- **(a)** Rename the Rust struct fields to camelCase (`supportsConfigurationDoneRequest: bool`). Bad: clippy enforces snake_case for fields by default; you'd be writing `#[allow(non_snake_case)]` on every field, fighting the language.
- **(b)** Per-field annotation: `#[serde(rename = "supportsConfigurationDoneRequest")] pub supports_configuration_done_request: bool` on every single field. Works, but duplicates the same transformation 50× for the full DAP `Capabilities` (~50 boolean fields). Drift surface — typo any one of those rename strings and that single field silently fails.
- **(c)** A single struct-level annotation that says "for *all* fields on this struct, convert snake_case to camelCase when matching JSON." One line, no drift.

(c) is the right move. Literal syntax:

```rust
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub supports_configuration_done_request: bool,
    pub supports_function_breakpoints: bool,
    pub supports_conditional_breakpoints: bool,
}
```

That single `#[serde(rename_all = "camelCase")]` on the struct tells serde: when matching JSON keys to struct fields, transform every Rust field's name to camelCase first. Drop it in, the test goes green. Run it yourself when you reach the artifact section to confirm.

> **Pocket this:** struct-level `rename_all` is the right default when *every* field follows the same casing rule. Per-field `rename = "..."` is for the *exception* — the one field that breaks the pattern (you'll see one in 60 seconds). Use both, but use the right one for the right job.

`rename_all` accepts other casings too: `"snake_case"`, `"SCREAMING_SNAKE_CASE"`, `"kebab-case"`, `"PascalCase"`, `"lowercase"`, `"UPPERCASE"`. Pick whichever your protocol uses.

---

## Concept 3 — The response wrapper: generics + per-field rename + `Option<T>`

`Capabilities` is just the *body* of an `initialize` response. The full DAP response is bigger — there's metadata around the body: `seq`, `request_seq`, `command`, `success`, etc. And here's the twist: **the body shape varies per command.** `initialize` → `Capabilities`. Later, `stackTrace` → a frames list. `setBreakpoints` → verified-breakpoint info. The wrapper is the same; the body type differs.

> 🤔 **Q:** In TypeScript, how would you type a response wrapper where the metadata fields are fixed but the `body` field's shape depends on which command you're handling? Sketch the interface.

<details>
<summary>Click after you've answered</summary>

Most people reach for: `interface DapResponse<R> { seq: number; command: string; success: boolean; body?: R }`. Type parameter on the generic, instantiated per call: `DapResponse<Capabilities>`, `DapResponse<StackTraceBody>`, etc.

Same pattern in Python (`Generic[R]`) and Java (`DapResponse<R>`).

</details>

The Rust shape is the same idea:

```rust
#[derive(Debug, Deserialize)]
pub struct DapResponse<R> {
    pub seq: i64,
    pub request_seq: i64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub command: String,
    pub success: bool,
    pub message: Option<String>,
    pub body: Option<R>,
}
```

Three new things to land:

**1. `<R>` type parameter.** Same as TypeScript generics. `DapResponse<Capabilities>` instantiates `R = Capabilities`; the `body` becomes `Option<Capabilities>`. Serde's macro auto-emits the `R: Deserialize` bound for us — you don't write it manually. (Without the macro, you'd need `where R: for<'de> Deserialize<'de>` somewhere; the macro handles the boilerplate.)

**2. `#[serde(rename = "type")]` — per-field rename.** Why this case and not `rename_all`?

- `type` is a **Rust keyword**. You literally cannot name a struct field `type` — the parser refuses.
- So the Rust field is `message_type` and we surgically tell serde to map JSON's `type` key to it.
- This is the use case for per-field annotation: when *one* field breaks the pattern, struct-level when the *whole struct* follows a uniform rule.

**3. `Option<String>` and `Option<R>` — the optionality exception to the "missing field = error" rule.** Earlier we established missing JSON fields are a hard error by default. Here's the exception worth memorising: **if the Rust field type is `Option<T>`, a missing JSON field becomes `None`, not an error.** Serde sees `Option<T>` as inherently optional because the type itself encodes "may not exist." Used everywhere; you'll see it on every "may be missing" field.

> 🔮 **Predict:** Notice this struct does *not* have `#[serde(rename_all = "camelCase")]`. (`Capabilities` did; this one doesn't.) What does that omission tell you about how DAP names its *response wrapper* fields on the wire?
>
> Specifically: when codelldb sends back a response, will the JSON contain `request_seq` or `requestSeq`?

<details>
<summary>Click after you've predicted</summary>

No `rename_all` rule means serde uses field names verbatim, so for `request_seq: i64` it expects JSON key `"request_seq"` — snake_case in both directions.

You actually saw this live in chapter 05's printed output — the JSON had `"request_seq": 1` right next to `"supportsConditionalBreakpoints": true`.

**DAP is bilingual about casing.** The protocol envelope (`seq`, `request_seq`, `type`, `command`, `success`) is snake_case. The bodies (`Capabilities`, `StackTraceBody`, etc.) are camelCase. Different conventions per layer.

Serde's per-struct attributes let us model that naturally — `Capabilities` gets `rename_all = "camelCase"`, `DapResponse<R>` doesn't, no global config to fight. Each struct owns its own naming rule.

</details>

---

## Try it yourself — round 3

You've watched me write `Capabilities` (round 1: I do) and `DapResponse<R>` (round 2: we do, with you predicting). Now you write `InitializeArgs` from scratch.

We're now sending the request side: typed `InitializeArgs` instead of the `serde_json::json!({...})` blob from chapter 05. New wrinkle: this is `Serialize` (going outbound), the mirror of `Deserialize`. Same derive shape, opposite direction.

But there's a real-world DAP gotcha that makes this exercise meatier than just "swap the derive."

The DAP spec wants these *exact* field names on the wire:

| Wire (JSON) | Rust idiom (snake_case) |
|---|---|
| `clientID` | `client_id` |
| `clientName` | `client_name` |
| `adapterID` | `adapter_id` |
| `linesStartAt1` | `lines_start_at1` |
| `columnsStartAt1` | `columns_start_at1` |
| `pathFormat` | `path_format` |
| `locale` | `locale` |

Look closely. **Most fields are clean snake → camel** (`lines_start_at1` → `linesStartAt1`). **Two are weird** — `clientID` and `adapterID` have uppercase `ID`, but `rename_all = "camelCase"` produces `clientId` (lowercase d). Mismatch. (`clientName` cleanly snake→camels to `clientName`; no override needed.)

Your task: write `InitializeArgs` in `crates/dap/src/types.rs` with the *combination* of annotations needed to land all seven names correctly.

You'll need:

- `#[derive(Debug, Default, Serialize)]` (and `serde::Serialize` in the import — you currently only import `Deserialize`)
- A struct-level rule for the bulk
- Per-field overrides for the two ID-suffixed ones
- A test that builds an instance, serialises it with `serde_json::to_string(&args)`, and asserts the **string output** contains the right wire-format names

Hint on the override: per-field `#[serde(rename = "...")]` *overrides* the struct-level `rename_all` for that field. Both annotations on the same struct, composing.

For the field types, use `Option<String>` for the string fields (most are optional in DAP), `bool` for the booleans. The `path_format` field is a string (`"path"` or `"uri"`), not a bool — that's the most common type bug in this exercise.

Write it. Run `cargo test -p lazydap-dap`. Iterate against the compiler. When it goes green, move on.

---

## Compiler conversation — three errors to expect, in order

Three classes of error are nearly universal on this exercise. Rather than pre-empting them, work through them as they arrive — the compiler is the curriculum.

### Error 1 — `&` on `to_string`

```rust
let json = serde_json::to_string(InitializeArgs { ... })?;
```

Compiler:

```
error[E0308]: mismatched types
   |
   | let json = serde_json::to_string(InitializeArgs { ... })?;
   |                                  ^^^^^^^^^^^^^^^^^^^^^^^ expected `&_`, found `InitializeArgs`
   |
help: consider borrowing here
   |
   | let json = serde_json::to_string(&InitializeArgs { ... })?;
   |                                  +
```

`serde_json::to_string<T>(value: &T)` takes a borrowed reference. Add the `&`. The compiler is literally telling you the fix.

### Error 2 — `&str` literals don't fit `Option<String>`

```rust
let args = InitializeArgs {
    client_id: "lazydap",
    ...
};
```

Compiler:

```
error[E0308]: mismatched types
   |
   |     client_id: "lazydap",
   |                ^^^^^^^^^ expected `Option<String>`, found `&str`
```

`"lazydap"` is `&'static str`, not `Option<String>`. To fit, wrap in `Some(...)` and convert the `&str` to `String` via `.into()`:

```rust
client_id: Some("lazydap".into()),
```

(`.into()` works because there's a `From<&str> for String` impl. It's idiomatic for short literals; `String::from("lazydap")` is equivalent if you prefer the explicit form.)

### Error 3 — JSON equality ≠ string equality

The most conceptually interesting failure. After fixing the compile errors, the test runs and panics:

```
assertion `left == right` failed
  left: "{\"clientID\":\"1234\",\"linesStartAt1\":true,...}"
 right: "{\n            \"clientID\": \"1234\",\n            \"linesStartAt1\": true,\n            ...\n        }"
```

The *serialisation* worked perfectly — read the LEFT side, every wire-format name is what DAP wants. What failed is the **assertion strategy**.

`assert_eq!(actual, expected)` compares two strings character-by-character. But JSON has **insignificant whitespace** — `{"a":1}` and `{"a": 1}` and the indented multi-line version are all the same JSON, but they're different character strings. JSON equality ≠ string equality.

Two right tools for comparing JSONs:

1. **Substring-match the wire-format names you care about** — rough, transparent, perfect for verifying that specific keys appear in the output:

   ```rust
   let json = serde_json::to_string(&args)?;
   assert!(json.contains(r#""clientID":"lazydap""#), "got: {json}");
   assert!(json.contains(r#""adapterID":"lldb""#), "got: {json}");
   assert!(json.contains(r#""linesStartAt1":true"#), "got: {json}");
   // negatives prove the renames REPLACED the defaults, didn't just ADD them:
   assert!(!json.contains("client_id"));
   assert!(!json.contains(r#""clientId""#));
   ```

   The `, "got: {json}"` second arg is a custom panic message — if the assertion fails, it prints the actual output. Way more useful than "assertion failed."

2. **Parse both sides into `serde_json::Value` and compare those** — proper semantic equality (`Value`'s `PartialEq` ignores whitespace, and for objects, ordering — JSON object key order is technically undefined). What you'd write in production for a deep equality check.

Substring is the better teaching tool because it's transparent about what's being checked. `Value`-comparison is the better production tool because it's semantically correct.

> **Bank this:** any time you compare two pieces of JSON, ask "am I checking the *string representation* or the *semantic content*?" Choose the tool accordingly. String-level checks are fragile to whitespace, ordering, and trailing newlines. Semantic-level checks need to parse first.

---

## What you can run now

```bash
cargo test -p lazydap-dap
```

Output:

```
running 3 tests
test types::tests::deserialises_a_capabilities_body ... ok
test types::tests::deserialises_a_full_initialize_response ... ok
test types::tests::serialises_a_initialize_args_body ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Three independent proofs:

- `Capabilities` deserialises from camelCase JSON — `rename_all` covered the whole struct.
- `DapResponse<Capabilities>` deserialises from a real DAP wire shape — bilingual envelope (snake) + body (camel) handled cleanly because each struct has its own per-struct rule. Plus the generic `<R>` instantiated, plus `Option<T>` correctly handled the absent `message` field as `None`.
- `InitializeArgs` serialises *outbound*, mixing struct-level `rename_all` for the clean fields and per-field `rename` overrides for `clientID`/`adapterID`. Both annotations composed without conflict.

The meatier demo is the **call-site diff** between chapter 05 and now.

**Chapter 05 — untyped access:**

```rust
let value: serde_json::Value = serde_json::from_slice(&body)?;
let supports_config_done = value["body"]["supportsConfigurationDoneRequest"]
    .as_bool()
    .unwrap_or(false);
//   ^^^^^^^^ runtime hash-map walks, no completion, typo silently → false
```

Three runtime lookups, each can return `Value::Null`. A typo in `"supportsConfigurationDoneRequest"` (`"supportsConfigDoneRequest"`? extra space? wrong capital?) compiles fine and silently gives you `false` at runtime. `as_bool()` on a non-bool returns `None`, masked further by `.unwrap_or(false)`. **Bug class: silent miscompile.**

**Chapter 06 — typed access:**

```rust
let resp: DapResponse<Capabilities> = serde_json::from_slice(&body)?;
let supports_config_done = resp.body
    .expect("body present on success")
    .supports_configuration_done_request;
//   ^^^ compile-time-checked field name. Typo → error[E0609]: no field
//       named `supports_configuration_done_requst`
```

One field access. IDE autocompletes the field name. Typo is a compile error with a "did you mean" suggestion. The `Option` on body is *honest* — you have to acknowledge that body could be missing (e.g., on an error response). The `bool` is a `bool`, not a probabilistic JSON value with five possible variants.

**Ladder check:**

- Chapter 04: spawned codelldb, read its first stderr chunk.
- Chapter 05: parsed one framed DAP message from the byte stream.
- **Chapter 06 (today): the bytes-to-typed-struct pipeline.** Same JSON, same wire, same parser underneath — but the call site is now compile-time-safe and IDE-driven.

What's still ahead (chapter 07, M2-2): turning these types into a reusable **transport** — a `DapTransport` struct that owns the codelldb child + TCP stream and exposes a generic `async fn request<T: Serialize, R: DeserializeOwned>(...) -> Result<R>` method. That's where generics on *methods* (vs generics on *types*), `AtomicI64` for sequence numbers, and `thiserror` for proper error types land. Today set up the types; next chapter puts them in motion.

---

## Teach-back

Before moving on, answer these in your own words. If you stumble, re-read the relevant section.

> 📣 **Q1:** What does `#[derive(Deserialize)]` actually *do* at compile time? Compare it to writing a Zod schema in TypeScript — what's the same, what's the substantive difference?

> 📣 **Q2:** "Missing JSON fields are a hard error by default" was the rule I gave you. Then `DapResponse.message` (typed `Option<String>`) deserialised cleanly from a JSON payload that didn't include a `message` key, and you got `None`. Reconcile those two statements — what's the rule, what's the exception, and why does the exception exist?

> 📣 **Q3:** When do you reach for `#[serde(rename_all = "camelCase")]` (struct-level) versus `#[serde(rename = "clientID")]` (per-field)? Frame it as a rule of thumb someone else could apply without thinking about your specific code.

---

## Pain anchors covered

| New construct | The pain it solves | In which language |
|---|---|---|
| `#[derive(Deserialize)]` | TS's `JSON.parse(s) as MyType` is a *lie* — type erased at runtime, no validation. Zod adds runtime checks but at the cost of two sources of truth (TS type + Zod schema). Pydantic unifies but requires inheriting from `BaseModel`. Serde derive generates the parser *from* the plain Rust type — single source. | TS, Python (partially) |
| `#[serde(rename_all = "camelCase")]` | Manual mapping of every wire-format key to every Rust field name. Without this rule, you'd write 50× duplicated annotations for a 50-field struct, with drift surface on every one. | Any language with idiomatic-vs-protocol casing mismatch |
| `#[serde(rename = "...")]` per field | The exception case — one field that breaks the bulk rule (e.g., a Rust keyword like `type`, or an irregular protocol convention like `clientID` with capital ID). | Any protocol with locally inconsistent naming |
| `Option<T>` for optional fields | The "missing field = silent default" trap. JS treats missing fields as `undefined`; Python treats them as `KeyError` if you use `dict[]` or silently `None` if `.get()`. Both lose type-system signal that "this field MAY not exist." Rust's `Option<T>` makes optionality part of the type — the compiler forces you to handle the `None` case. | C (NULL pointer), JS (undefined), Java (null), Python (KeyError vs `.get()`) |
| Generic struct `DapResponse<R>` | The "wrapper has fixed shape, body shape varies per command" pattern. Without generics: repeat the wrapper per command (`InitializeResponse`, `StackTraceResponse`, ...) with copy-paste fields. With: one definition, parameterised at the call site. | Same in TS/Python/Java; serde's wrinkle is that the macro auto-generates the bound `R: Deserialize` so you don't write it. |

---

## A note on the smoke test

The three `#[cfg(test)] mod tests` blocks at the bottom of `crates/dap/src/types.rs` serve as smoke tests for the chapter's promise. Run them with:

```bash
cargo test -p lazydap-dap
```

Each test asserts on the chapter's outcome promise:

- The deserialise test asserts that camelCase JSON populates snake_case Rust fields with the right values — verifying `rename_all` did its job.
- The full-response test asserts that the bilingual envelope+body pattern works end-to-end with a generic `DapResponse<Capabilities>` — verifying generics, per-field rename for `type`, and `Option<T>` optionality all compose.
- The serialise test asserts that the *outbound* JSON contains the exact wire-format names DAP expects — verifying `rename_all` + per-field `rename` overrides compose for the request side.

The test syntax (`#[test]`, `#[cfg(test)]`, `serde_json::to_string` and `from_str`) is treated as deferred-load infrastructure — same as `#[tokio::main]`. We'll dig into how testing works in the dedicated chapter (TDD-1) before the protocol crate's codec lands. The test exists so that if a future you refactors these structs and breaks the wire-format mapping, the regression check catches it before downstream chapters silently start producing wrong DAP messages.

The tests assert on the chapter's *promise* (`clientID` appears, `clientId` does not) — not on internals. You could rewrite the structs using a custom `Deserialize` impl, a different rename strategy, or any other approach, and the tests would still pass. Tests should constrain behaviour, not creativity.

---

## See also

- ← [Chapter 05: Read one message](05-read-one-message.md)
- → Chapter 07: DAP transport and seq *(coming soon)*
- [Underlying milestone: M2 — Initialize handshake](../implementation/tasks/M02-initialize-handshake.md)
- [`serde` documentation](https://serde.rs/) — the official derive guide; the section on [field attributes](https://serde.rs/field-attrs.html) lists every attribute serde understands
- [`serde_json` documentation](https://docs.rs/serde_json/) — `Value`, `from_*`, `to_*`
