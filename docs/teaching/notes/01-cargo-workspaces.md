---
chapter: 1
session_id: WS-1
title: Cargo workspaces
sessions_run:
  - date: 2026-05-01
    learner: Bhekani
    duration_minutes: 60
    notes_below: true
---

# Teaching notes — Chapter 01: Cargo workspaces

## Concept anchor

The chapter teaches **Cargo workspaces** as a *monorepo* concept (not a single-package one), with the four pillars at the root (`[workspace]`, `[workspace.package]`, `[workspace.dependencies]`, `[workspace.lints]`) and **opt-in field-by-field inheritance** as the model member crates use to consume the root.

If a future session runs over scope, the most likely creep is into `[workspace.dependencies]`. Adding a dep to the root and inheriting it in a member is half of chapter 02's content (where it's introduced naturally with `tokio`). Resist demonstrating it in chapter 01.

## Common wrong predictions

| Predict question | Common wrong answer | Why learners go there | How the chapter currently calibrates |
|---|---|---|---|
| "How does a Cargo workspace differ from a single Cargo.toml at the root?" | "It's basically `npm init` — `Cargo.toml` is `package.json`, `Cargo.lock` is the lockfile, the workspace is just convention on top." | They map `Cargo.toml ≈ package.json` correctly, then *don't* notice that npm has a separate **workspace** concept (npm workspaces / pnpm workspaces / yarn workspaces) on top of single-package layouts. The conflation feels intuitive because `npm init` is the move they've made 100 times. | First `<details>` block: reframes the analogy as "npm/yarn/pnpm workspaces, plus pnpm catalogs for inheritance." |
| "Will an empty `lib.rs` compile?" | "No — there's nothing to compile, the file is empty." | Mental model from C: "a `.c` file with no functions is useless and probably won't link." Or from JS: "a module that exports nothing is suspicious." | "Step 4 — Make the empty crate compile" surfaces this directly: empty lib OK, empty bin no, with the linker reasoning. |
| "What does `publish = false` do?" | "Sets a default version policy" / "Disables something publish-related but I'm not sure what" | Most readers haven't published a crate to crates.io and don't know what publishing entails by default. | Explained inline as "this crate is private to the workspace; `cargo publish` will refuse." |

## What surprised the learner

- **The empty-`lib.rs`-compiles fact** was a small but useful surprise. The mental-model bug ("the file is empty, so nothing to compile, so it might fail") got surfaced and corrected before more complex builds, exactly as predict-before-run is meant to do.
- **The pnpm `catalog:` analogy for `[workspace.dependencies]`** landed cleanly. Worth reusing for any future explanation that needs an npm-world hook for a Rust ergonomic.
- **mxr being hybrid, not virtual** caught the learner mildly off-guard. They assumed mxr would be the canonical "follow this" reference, then noticed lazydap chose differently. Use this as a teaching moment: the right answer is project-shape-dependent, not "copy the existing thing." Both shapes are legitimate.

## Sticky points (concepts that needed a second pass)

- **Inheritance is opt-in field-by-field, not auto-copy-paste.** The teach-back response framed it as "almost like it just copy pastes all the properties into its own cargo.toml". Close, but the *opt-in* part needed reinforcing. The framing "workspace = provider, member = subscriber, inheritance = subscription opted into per-field" is the better model. Surface this explicitly in the surface-your-model section if a future learner says the same.

## Refinement ideas

- [ ] Add an explicit predict question on `version.workspace = true` vs `version = "0.1.0"` (literal) to reinforce opt-in. Currently the predict question is there but folded into the read-through; lifting it to a numbered prediction would make the contrast sharper. Apply when 1+ more learner gets confused by the "copy paste" framing.
- [ ] Consider adding a tiny diagram (text art) showing the four pillars of the root manifest with arrows pointing to which fields a member can opt into. Apply if any future session reports the four-pillars framing didn't land.
- [ ] The "Try it yourself" exercise (add a second crate `lazydap-protocol` then delete it) is a bit throw-away. Consider replacing with "write the manifest for a second member crate and make it work without running cargo" — pure prediction exercise, no compile feedback. Wait for 2+ learners' feedback on whether the current shape lands.

## Notes for future sessions on this chapter

- **The "you do" round (writing `crates/core/Cargo.toml`) worked cleanly.** The learner produced something *better* than the mxr reference (included `rust-version.workspace = true` which mxr's own crate omits). This pattern, "here's the template, write the analogous file", is high-yield when there's an obvious reference. Keep it for similar future sessions.
- **The middle "we do" round was implicit.** The conceptual stakes were modest enough that I-do then you-do collapsed naturally. Watch for sessions where this is appropriate (mechanical, declarative content) vs ones where the middle round is essential (ownership, lifetimes, anything with a genuinely tricky decision).
- **Anchor on mxr aggressively.** The learner has it open in another window and refers to it constantly. Don't just mention it once. Say "mxr does X here, lazydap does Y, here's why" at every meaningful divergence. Mxr is half the curriculum.
- **Don't introduce `[workspace.dependencies]` content in WS-1.** It's tempting because the table is *there*. Save it for WS-2 where `tokio` arrives with a concrete need.

## Did the artifact land?

Session 2026-05-01 (Bhekani): Yes. `cargo build --workspace` succeeded; the `0.1.0` in the build output came from `[workspace.package]` confirming inheritance worked end-to-end. The learner could see the chapter's promise delivered.

**Caveat**: framing-of-the-artifact landed weakly. Per the post-session addendum on WS-3, the learner reported the early sessions felt like "piling concepts toward an arrival point." WS-1 *did* produce an artifact (a building workspace) but the close didn't celebrate it as one. **Future runs**: end the session with "watch: `cargo build --workspace` works now, and `cargo metadata` lists your member. That's something you couldn't do an hour ago." Make the artifact visible.

## Reuse log

(Empty — first run only. Update on next teach.)
