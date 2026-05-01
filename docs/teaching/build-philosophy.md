# Teaching mode build philosophy

Each teaching session must ship a **demonstrable runnable artifact**, not progress toward a future arrival point. Skateboard → scooter → bike → motorbike → car. Stop after any session and there's a working thing.

This mirrors and extends `docs/blueprint/14-roadmap.md`'s "Build philosophy" section, which already states the principle for full milestones. This doc applies it to **teaching sessions**, which are smaller units than milestones (some milestones split into 2–5 sessions).

## The principle

> Every teaching session must produce something the learner can run and show. Not a concept piled toward a future arrival, but a real artifact — small if necessary, but real.

Why: senior-engineer learners have strong intrinsic motivation but high opportunity cost. "Concepts piling toward a future arrival" reads as stalling, even when the concepts are landing. Visible progress is the fuel that keeps slow learning sustainable.

## What counts as an artifact

Anything the learner can run and demonstrate:

- A CLI that prints something based on input.
- A test that passes.
- A function that returns the right value when called from a test or REPL.
- A binary that connects to an external thing and prints what it gets.
- A flag added to an existing CLI that changes behaviour visibly.

The bar is **demonstrable** + **incremental over the previous session**. Small is fine. Real is the requirement.

## The session-open / session-close ritual

**Open every session** by stating the artifact:
> "Today's concept is X. By the end of this session you'll have Y you can run."

**Close every session** by demonstrating the artifact:
> "Here's what you can do now that you couldn't an hour ago. Run it." [run it]

**Make the ladder visible** at session close:
> "Last session you had a CLI that prints. Now you have a CLI that connects to a debug adapter and prints its greeting. Two sessions from now you'll have a CLI that drives a full DAP launch."

Do not make the learner run the artifact themselves to feel motivated. That step is part of the session, not homework.

## The ceremony exception

Some sessions are unavoidable ceremony — workspace setup, license decisions, CI configuration. They produce no user-visible artifact. In these sessions:

- **Name the ceremony at open**: "Today is workspace setup — there's nothing user-visible to show at the end. But here's what's now possible because of it..."
- **Close by stating what's now possible**, not what was demonstrated.
- **Pull a tiny artifact forward where possible**: even ceremony sessions can usually include a one-liner like "run `cargo fmt --check` and watch it pass" as a minimal demo.

The lazydap WS-1, WS-2, WS-3 sessions are illustrative: WS-1 was pure ceremony (workspace structure), WS-2 produced a small artifact (`cargo run -p lazydap-daemon -- --message hi` works end-to-end), WS-3 was mostly ceremony (CI / conventions / license) with a small artifact at the end (the four CI checks all passing locally and the first commit landing).

## Reconciliation with rule #12 (slowness)

The teaching skill's rule #12 says slowness is the goal — resist racing. Rule #13 (this principle) says every session ships an artifact. They aren't in conflict:

- **Rule #12 governs pace within a session** — don't race through concepts.
- **Rule #13 governs deliverables across sessions** — don't pile concepts without artifacts.

A slow session can ship a small artifact. A 90-minute session might produce a 5-line CLI command. That's correct. The artifact's size scales with the session's pace, not with raw time spent.

## Failure mode to watch for

When you finish three sessions in a row without an explicit demonstration at session close, the rule was violated. Pull the artifact forward in the next session — even a one-line behavior change is enough. The visibility matters more than the size.

If the learner asks "where is this going?" — that's the signal that the cumulative narrative has gone fuzzy. Re-anchor by demonstrating what they can run today vs. last session.

## See also

- [`docs/blueprint/14-roadmap.md`](../blueprint/14-roadmap.md) — the canonical statement of this philosophy at the milestone level.
- [`docs/teaching/sessions.md`](sessions.md) — the per-session teaching plan; each row should produce an artifact.
- The portable `teaching` skill at `~/.dotfiles/.agents/skills/teaching/SKILL.md` — rule #13 in the operating rules; expanded in `references/operating-rules.md`.
- Obsidian: `[[Skateboard MVP]]` and `[[Shippable Increment Per Session]]` — the underlying concepts.
- Obsidian: `[[Teaching Senior Engineers]]` — the synthesis hub for the full pedagogy.
