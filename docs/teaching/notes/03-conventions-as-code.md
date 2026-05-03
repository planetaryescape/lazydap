---
chapter: 3
session_id: WS-3
title: Convention as code
sessions_run:
  - date: 2026-05-01
    learner: Bhekani
    duration_minutes: 45
    notes_below: true
---

# Teaching notes — Chapter 03: Convention as code

## Concept anchor

The chapter teaches **convention as code**: pinning shared expectations (toolchain, formatter, linter parameters, license, CI) as files that travel with the repo. The chapter is **lighter conceptually** than 01 and 02; the only real conceptual lift is the `clippy.toml` vs `[workspace.lints.clippy]` split (parameters vs levels). The rest is mechanical, but the *why-each-file-exists* framing is the conceptual hook.

This is also the chapter where rule 13 (every session ships a demonstrable artifact) was *retroactively* paid for in pain. See the post-session addendum on the source Obsidian note for WS-3. The artifact for this chapter is "all four CI-equivalent commands pass locally + first commit lands." Make sure that lands explicitly.

## Common wrong predictions

| Predict question | Common wrong answer | Why learners go there | How the chapter currently calibrates |
|---|---|---|---|
| "What four steps does Rust CI run?" | "Type check, build, lint, format" or "Lint, build, test". Close but missing one of the four. | The Node mental model has a separate `tsc --noEmit` type-check step before the bundle. Rust collapses type checking into compilation, so the slot most learners reserve for "type check" is a false position. Tests, conversely, often get omitted because in some Node flows tests are a separate concern. | The surface-your-model `<details>` block lists the canonical four (fmt, clippy, check, test) and explains why no separate type check (it's part of compile) and why tests *are* canonical (not optional). |
| "Why are `clippy.toml` and `[workspace.lints.clippy]` separate files?" | "Different scopes, one's workspace-wide, one's per-crate" / "One's older, one's newer." | The naming similarity suggests they answer the same question. The actual split is by *kind of decision*: levels vs parameters. | Inline `<details>` block: levels go in `[workspace.lints.clippy]`, parameters (MSRV, thresholds) go in `clippy.toml`. Two questions, two files. |
| "Will the four CI jobs run in order, or in parallel?" | "In order, cheapest first to fail fast." | Reasonable from a *local* `make check` mental model. The "fail fast cheap-to-expensive" framing is correct for local sequential pipelines but not for GitHub Actions which spreads jobs across runners. | Inline `<details>` block: parallel by default, fail-fast still works because the slowest-failing job reports independently. The `concurrency:` block adds a different kind of fail-fast (cancel stale runs on the same branch). |
| "Will all four commands pass locally on first try?" | "Yes" | The empty-file-needs-newline gotcha catches most projects coming out of `cargo init` + add-empty-files-by-hand workflows. | The chapter walks through the rustfmt failure, fix, re-run flow — designed to surface the gotcha so it lands as a learning moment, not a frustrating CI red. |

## What surprised the learner

- **`rust-toolchain.toml` is *active*, not passive.** The auto-switch (rustup reads it on `cd`) was framed as new information; the learner had assumed it would be `.nvmrc`-style "you have to remember to switch." Worth explicit calling out. The active vs passive distinction is small but it removes a class of friction.
- **Tests in CI weren't predicted.** Three of four steps named, missing tests entirely. Worth pre-empting: when surfacing the model, ask explicitly "do you think tests are part of standard CI for Rust?"
- **The empty-file-needs-newline rustfmt failure** was a useful low-stakes "here's how CI catches things" moment. Reuse this as a recurring example for any "what does CI actually catch over local trust-yourself" question.

## Sticky points (concepts that needed a second pass)

- **`clippy.toml` was initially understood as "version of clippy" rather than "parameters for individual clippy lints."** The learner's first teach-back conflated the file with MSRV-as-version-pinning. The refinement landed but took a second pass. Future runs: state up front "`clippy.toml` is for *parameters*, not versions; MSRV is one such parameter" before showing the file.
- **CI ordering**: the learner's prediction was "cheap to expensive, sequential, fail fast", correct *intuition* for a local script but wrong for our GitHub Actions setup (parallel by default). Worth pre-empting in the surface-your-model section: "predict whether they run sequentially or in parallel."

## Refinement ideas

- [ ] Add an explicit pre-emption: state at session open that this session is **light** and the artifact is "all four CI-equivalent commands pass locally + first commit lands", so the conceptual lightness doesn't read as "we're not really doing anything." Apply immediately on next teach.
- [ ] Consider adding a small rustfmt diff exercise: introduce a deliberate formatting violation (extra spaces, wrong indentation), run `cargo fmt --all -- --check`, see the diff, run `cargo fmt --all`, watch it disappear. Currently the chapter handles this implicitly via the empty-file gotcha; an *intentional* exercise would make the formatter feel useful rather than just gate-keeping. Wait for 1+ learner feedback.
- [ ] The `.cargo/config.toml` exercise (try-it-yourself) is more of a thought exercise than a build exercise. Consider promoting it from "your turn" to a sidebar. It doesn't fit the make-something-run pattern of the other chapters' exercises.
- [ ] If `cargo install just` is feasible, consider adding a `justfile` with a `check` target that runs the four commands sequentially (the local fail-fast pattern). Defers to chapter 04+ if useful.

## Notes for future sessions on this chapter

- **The user reported feeling under-stimulated rather than fatigued** at end of WS-1+WS-2+WS-3 stacked. The conceptual lift was light by design (workspace setup, not language fundamentals). The default mental model "pace M0 cautiously because we just did three sessions" was wrong. The three were mechanical, not load-bearing. **Pace M0 normally. Don't conflate session count with cognitive load.** Measure cognitive load by concepts-per-session, not sessions-per-day.
- **The "we did three sessions in one sitting" framing made the learner feel they were piling concepts toward an arrival point.** This is what spawned operating rule 13 (every session ships a demonstrable artifact). For future runs: open WS-3 explicitly with "today's artifact is your first green CI run + first commit landing, let's get there." Close with running the four commands live and showing the commit on GitHub.
- **The `.nvmrc` / Prettier / ESLint surface-your-model anchoring landed cleanly.** Continue with this anchor-on-prior-tooling pattern when introducing Rust-specific tooling in later chapters.
- **Don't predict-before-run on every mechanical step.** This learner is comfortable with one-shot batched-execution when the conceptual ground is mostly mapped. Reserve predict pauses for moments where wrong predictions would expose mental-model bugs (the clippy-split, the CI parallelism, the empty-file gotcha). Skip them on the literal `mkdir`/`touch` commands.

## Did the artifact land?

Session 2026-05-01 (Bhekani): Yes. All four commands passed locally after fixing the rustfmt empty-file complaint, and the first commit (`6a06e68 chore: initial workspace scaffold`, ED25519-signed) landed and pushed.

**Framing caveat**: across all three sessions on the day, the artifact-celebration step was missing from session-close. The learner explicitly opened the terminal *themselves* between sessions to feel like progress had happened. That's a rule-13 violation. The post-session addendum captures the framing change for M0 onward. Apply retroactively to WS-1, WS-2, WS-3 when these chapters are run live again. Close with the demo command, name the ladder, don't let the learner discover their own progress.

## Reuse log

(Empty — first run only. Update on next teach.)
