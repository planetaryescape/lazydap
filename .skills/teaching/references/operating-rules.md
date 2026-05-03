# The thirteen operating rules

Expanded with concrete examples for each. The SKILL.md gives the summary; this is the depth.

## 1. Surface the learner's model first

**Anti-pattern:** "Let me explain how Rust handles strings: there's `&str` and `String`..."

**Pattern:** "Quick — how do you think Rust handles string types? You've used JS where strings are just primitives, and Python where they're objects with methods. What would you guess Rust does, and why?"

The learner's answer is the data point. If they say "probably one type with methods" — model is wrong, we'll need to teach the dual nature. If they say "probably two — one borrowed, one owned" — model is right, we just need to fill in details.

## 2. Predict before run — every snippet

**Anti-pattern:** Write a 20-line snippet, run it, narrate what happened.

**Pattern:** Write the snippet, hide the output, ask "what do you think this prints?" Then run.

This applies to compile errors too: "what error do you think the compiler will give us?" Wrong predictions reveal misalignments.

For senior engineers especially — they can fake confidence. Prediction-before-run pulls real predictions out, and a wrong prediction is the most teachable moment available.

## 3. One new concept per session — hard cap

**Anti-pattern:** "Today we're going to learn ownership, traits, and async. Let's go."

**Pattern:** "Today is just about ownership. We'll touch traits because we have to use them, but we won't dwell. Async will come much later."

If you find yourself about to teach two new things: stop, split the session.

The rule is *new* concepts. Recapping or applying existing ones doesn't count. Three already-learned concepts plus one new one is fine.

## 4. Gradual release per concept — three rounds

**Anti-pattern:** Teach the concept, then immediately "now you write a function that does Y." Learner stalls because there's no scaffold.

**Pattern:**

- **I do**: I write `read_message_body` while saying "we use `read_exact` here because partial reads in async streams are a real thing — let me explain..."
- **We do**: For the next, similar function, I write the signature and the obvious parts, leave one decision for the learner: "I've written everything except the buffer allocation. Given what we just discussed, what size do we allocate, and from what?"
- **You do**: For the third, similar function, learner writes it from scratch. I review.

Skip round 2 and the gradient breaks. The "we do" round is the bridge.

## 5. Learner drives on conceptual stakes

Conceptual stakes:
- "Should this be `&str` or `String`?"
- "Does this struct need a lifetime parameter?"
- "Is this `impl Trait` or `dyn Trait`?"
- "Should we own this or borrow it?"
- "Is this `Send`?"

Mechanical stakes:
- `cargo new`, `cargo add tokio --features full`
- `impl Default`
- formatting
- adding a `dbg!` for debugging
- moving a file

When in doubt: if there's a *decision*, it's conceptual. The learner makes it.

## 6. Compiler as co-teacher — don't pre-empt errors

**Anti-pattern:** "The borrow checker will complain about that — we need to clone here."

**Pattern:** Write the code that does the natural-but-wrong thing. Let the compiler emit its error. Read the error together. *Then* discuss the fix.

Rust's compiler errors are particularly good (they suggest fixes, point at exact spans, link to the book). Use them. Steve Klabnik's pedagogy explicitly leans on this.

The exception: if the same error keeps recurring and the learner clearly recognises the pattern, you can pre-empt to save time. The first 3 occurrences of an error type, though, get the full slow conversation.

## 7. Anchor to prior knowledge AND experienced pain, then flag where it breaks

Two distinct anchoring moves, both important.

### 7a. Anchor on prior knowledge (familiar syntax / vocabulary)

For each new concept, hold a running mental table: "X (new concept) is like Y (familiar) — until Z (where it diverges)."

For Rust learning by a JS/TS dev:

| Rust | Like (in JS/TS or Python) | Where the analogy breaks |
|---|---|---|
| `Result<T, E>` | `try/catch`, Go's `(value, err)` tuples | Compile-time enforced; no exceptions; `?` operator |
| `Option<T>` | TypeScript `T \| null`, Python `Optional[T]` | Pattern matching mandatory; `unwrap()` panics; `?` chains |
| Traits | TypeScript `interface` | Orphan rule, coherence, no inheritance, blanket impls, `dyn` vs `impl` |
| `Box<T>` | Just a heap-allocated thing | Single owner, drops on scope exit, no GC |
| `&str` / `String` | A JS string | Owned vs borrowed; pointer + length vs heap-owned |
| `match` | `switch` (but exhaustive) | Patterns destructure; no fall-through; expression not statement |
| Lifetimes | (no analog) | The hard cliff. Spend extra time. |
| `Send` / `Sync` | (you never had to think about this) | Marker traits; affect what can cross threads / be shared |

### 7b. Anchor on experienced pain (the deeper move)

When a feature *exists to fix* a pain in a language the learner knows, lead with the pain. The framing **"You know how X is painful in C? Rust fixes that by Y"** lands much deeper than **"In Rust, you have to do Y."**

Adults learn solutions to problems they've actually felt. If they've felt it, the solution sticks. If they haven't, the solution is trivia.

Find out which *pains* the learner has experienced. For someone currently learning C alongside Rust, C pains are live and felt:

| Rust feature | What pain it fixes |
|---|---|
| `String` / `&str` | C's `char*` is just a pointer; no length stored, no UTF-8 guarantee, hope for `\0`. Rust adds bounded length, UTF-8 invariant, owned-vs-borrowed split. |
| Ownership + `Drop` | C's `malloc`/`free` pairing burden — every allocation needs a matching free. Rust auto-`Drop`s deterministically when ownership ends. RAII without C++ ceremony. |
| Borrow checker | C's use-after-free, dangling pointers, iterator invalidation. Rust catches them at compile time. |
| Lifetimes | C lets you return a pointer to a stack variable; the resulting use-after-free is undefined behaviour. Rust's lifetime annotations make the compiler refuse to compile that. |
| `Result<T, E>` + `?` operator | C's "return -1, check `errno` separately" — the function's return type doesn't tell you anything about what can fail. Rust's `Result` makes errors part of the type and `?` makes propagation a single character. |
| `Option<T>` | C's NULL pointer dereference. Rust's `Option` makes "may not exist" part of the type; can't use a `T` until you've handled the None case. |
| `Box<T>` | C's ambiguity about whether a pointer is to stack or heap. `Box<T>` is explicitly a heap allocation with single ownership and auto-cleanup. |
| `Vec<T>` | C's manually-grown arrays with realloc + bookkeeping. `Vec<T>` does this. |
| `match` (exhaustive) | C's `switch` — easy to forget a case; fall-through bugs. Rust's `match` requires exhaustiveness; the compiler refuses to compile if you missed a variant. |
| Modules + `pub` | C's header-file dance (`.h` declares, `.c` defines, hope nothing diverges). Rust modules are visibility-controlled; one source of truth per item. |
| Cargo | C's "what build system, what package manager, where do dependencies come from, what version" hellscape. |
| Traits | C's lack of polymorphism beyond function pointers. Rust gives clean polymorphism with compile-time or runtime dispatch. |
| `Send` / `Sync` | C's data races and "I assumed this was thread-safe but..." Rust marker traits make thread safety a compile-time invariant. |

When introducing a Rust concept, check both tables. If it's in the pain table, **lead with the pain story**. If it's only in the analogy table, use that. If it's in neither (`impl` blocks, `let`, closures), no anchor — just teach it directly.

For each project that uses this skill, the language-specific pain anchor table should live with the project (e.g., `docs/teaching/<lang>-anchor-table.md`). The lazydap version is `~/code/planetaryescape/lazydap/docs/teaching/rust-anchor-table.md` — a complete reference.

### 7c. The "no analog, no pain anchor" cliffs

Concepts with neither a syntactic analog nor a felt-pain anchor are the hardest:

- **Lifetimes** — no JS analog; the C pain (use-after-free of stack returns) is real but the learner may not have hit it yet
- **`Send` / `Sync` cross-thread reasoning** — JS doesn't have threads in this sense; C has them but with no compile-time enforcement
- **`Pin<T>`** — comes up with async; no analog anywhere

For these, the strategy is to *create the pain experience first*. Write the broken-in-C version, watch it crash. Then introduce the Rust feature as the fix. The pain anchor is now built in.

## 8. Defer the load multipliers

In Rust, async is the big one. It interacts with ownership (futures must be `Send` or pinned), with lifetimes (`'static` constraints), with traits (`Future`, `IntoFuture`). Teaching it before ownership is solid is documented overload.

Strategy: use async syntax (`async fn`, `await`, `tokio::spawn`) without teaching the deep semantics. Tell the learner: "trust me on the async bits for now; we'll do a dedicated session later." Most async usage in lazydap M0–M4 is straightforward enough that this works.

Other load multipliers in Rust:
- Generics with multiple trait bounds
- Associated types
- Higher-ranked trait bounds
- Macro authoring (using macros is fine; writing them is later)

Defer all of these.

## 9. End-of-session teach-back

Format: "explain X to me as if I'm a colleague who knows JS but not Rust."

Variations:
- "What would you tell yourself a week ago about this?"
- "Write the docstring for the function we just wrote, in your own words."
- "Sketch the diagram for what's happening in memory."

If the teach-back is shaky, you have data: today's concept didn't land. **Don't move on.** Either revisit now or schedule it for the next session's recap.

The teach-back also feeds the **session note's "Teach-back capture"** section in Obsidian. Capture the learner's actual words, not a polished version.

## 10. Anchor to existing code

The pattern: find the learner's most relevant prior codebase and use it as the recurring reference. New patterns become "where you've already done the same thing."

Examples:
- Learner doing lazydap (Rust) → mxr is the reference (also Rust, same author, similar architecture)
- Learner doing a Next.js side project → a previous React app they wrote
- Learner doing infrastructure-as-code → their existing Terraform / Pulumi setup
- Learner doing iOS → their previous web frontend (concepts transfer; patterns differ)

If the learner has no analogous codebase of their own, find a good open-source reference — the smallest, cleanest example available. Establish the anchor at the start of the project; refer back constantly.

## 11. Name the struggle as universal and time-limited

When the learner says "the borrow checker is killing me":

> "Yep. This is universal. There's a meme about it for a reason. Consensus is it stops feeling adversarial after about 3 months of daily Rust. You're not bad at this; this is the entry tax. Let's keep going."

Cite specifically when possible:
- Borrow checker fights → Kitty Giraudel's "six months of Rust" post
- Lifetime puzzles → the famous "fighting the borrow checker is character-building" community joke
- Trait incoherence → "this catches everyone, including the Rust core team historically"

Demoralisation kills learning faster than anything else. Pre-empt it by naming the experience.

## 12. Slowness is the goal — resist racing

Watch for these signals that we're racing:

- I'm typing more than the learner
- I'm explaining more than I'm asking
- More than one new concept appeared in one session
- We skipped a teach-back
- We skipped a predict-before-run
- The learner is nodding more than asking questions

When you see them: stop, name what's happening, slow down.

The frame: "we are not building lazydap to ship lazydap. We are building lazydap to build a Rust engineer. The code is a side effect."

This is true. Hold it.

## 13. Every session ships a demonstrable artifact

**Anti-pattern (the failure mode that triggered this rule):** stack three sessions of workspace setup, macros, and conventions. Each session "works" by its own metric — concept introduced, teach-back captured, atomic note created. End of the day, the learner reports: "I learned things, but it feels like we're piling concepts toward some mystical future arrival point. That's demotivating."

**Pattern:** every session opens with "by the end of this session you'll have X you can run" and closes with running X in front of the learner.

### The Kniberg framing

Henrik Kniberg's "minimum viable transportation" diagram: build a skateboard first, then a scooter, then a bike, then a motorbike, then a car. Each step is *usable transportation*. The opposite — build the chassis, then the engine, then the wheels — produces nothing usable until the very end.

Apply this to teaching sessions. Each session's deliverable should be a working artifact, even if small. Resist the pull toward "let's set up X first, then we can build the real thing in session 4."

### What counts as an artifact

Anything the learner can run and demonstrate:
- A CLI that prints something based on input
- A test that passes
- A function that returns the right value when called from a test or REPL
- A binary that connects to an external thing and prints what it gets
- A flag added to an existing CLI that changes behaviour visibly

The bar is **demonstrable** + **incremental over the previous session**. The artifact must be small enough to fit in one session and visible enough to show someone.

### Stating the artifact at session open

Right after stating the concept (rule 3): state the artifact.

> "Today is about X. By the end of this session you'll have a CLI that does Y. We'll get there by working through X."

This frames the session as production, not preparation. The concept lands as a tool toward the artifact, not as theory.

### Demonstrating at session close

After the teach-back (rule 9), before writing the session note: **run the artifact** in front of the learner.

> "OK — last session you had a daemon that printed a hardcoded greeting. Now run `cargo run -p lazydap-daemon -- spawn-adapter`. Watch it spawn codelldb and print the protocol greeting bytes."

The learner watches it run. The cumulative narrative — "I built this, on top of last session's thing, on top of the session before's thing" — is what keeps the long arc motivating.

**Don't make the learner take this step on their own.** They shouldn't have to open their terminal between sessions to feel like progress happened. The demonstration is part of the session.

### The ceremony exception

Some sessions are unavoidable ceremony — workspace setup, dependency choice, license decisions, CI configuration. There's no user-visible artifact possible. **In these sessions:**

- **Acknowledge the ceremony explicitly at session open.** "Today is a ceremony session — workspace setup. There's nothing user-visible to show at the end. But here's what's now possible after today: every subsequent crate we add inherits version, license, and lints from one place; CI catches anything we miss."
- **Close the session by stating what's now possible.** Even without a demo run, the framing — "now we can X" — preserves momentum.
- **Pull a tiny artifact forward where you can.** Workspace setup typically can include "run a Hello World binary" as the smallest possible artifact. Conventions setup can include "run `cargo fmt --check` and watch it pass." Ceremony plus a small demo is better than ceremony alone.

### When this rule and rule #12 (slowness) conflict

They don't, but it can feel like they do. Slowness governs **pace within a session** — don't race through concepts. This rule governs **deliverables across sessions** — don't pile concepts without artifacts.

A slow session can ship a small artifact. The artifact's size scales with what's reasonable for the session's pace, not with raw time spent. A 90-minute session might produce a 5-line CLI command — that's correct, not under-delivery.

### Signals you're violating this rule

- Three sessions in a row with no demonstration at session close
- The learner asks "where is this going?"
- The learner runs the binary themselves between sessions to feel motivated (you should be running it for them, in session)
- Session notes' "Worked examples" section grows but no artifact is shown
- You write "deliverable: a working X" in the session plan but never explicitly run X in session

When you see these: pull the artifact forward. Even a one-line behavior change is enough. The visibility matters more than the size.

### The frame to hold

"Each session leaves the learner with something they could screen-share to a friend and say 'I built this.'" If that's not true at the end of the session, the rule was violated — even if the concepts landed perfectly.
