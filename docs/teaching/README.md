# Teaching plan

Parallel to `docs/implementation/`. Same milestones, different lens.

## Why this directory exists

The milestone files in [`docs/implementation/tasks/`](../implementation/tasks/) are written for **ship-mode pace**: each one a self-contained unit of work an agent (or future contributor) can pick up and complete. They stay pristine on purpose — if the user ever wants to hand the project off to a coding agent that just builds without teaching, the task tracker works as-is.

This directory is the **teaching-mode overlay**. It takes the same milestones and slices them into sessions sized for the cognitive-load discipline of teaching mode (one new concept per session, hard cap). Some milestones are 1 session; some are 5.

The two views never get mixed. Implementation tracks tasks. Teaching tracks sessions. Both reference the same underlying milestones.

## What's in here

- [`sessions.md`](sessions.md) — per-milestone session breakdown. Plan only; sessions get logged separately.

## Where session logs live

**In Obsidian, not here.** The plan is here; the log lives in [`Lazydap Teaching Sessions.md`](file:///Users/bhekanik/Library/Mobile Documents/iCloud~md~obsidian/Documents/pkm/Lazydap%20Teaching%20Sessions.md) at vault root, with per-session children named `Lazydap Session YYYY-MM-DD.md`. The `obsidian` skill handles the conventions; the `teaching` skill captures sessions.

This separation matters: `sessions.md` here is **what we plan to do**; the Obsidian hub is **what actually happened**. Don't update the plan with completion data — that lives in the log.

## Workflow

1. Open [`sessions.md`](sessions.md). Find the next session for the current milestone.
2. Run the session per the `teaching` skill's operating rules.
3. End-of-session: capture the session in Obsidian (`Lazydap Session YYYY-MM-DD.md`) + extend any atomic concept notes.
4. Repeat. When all sessions for a milestone are complete, mark the parent milestone done in [`/TODO.md`](../../TODO.md).

## When to update this directory

- **Plan diverged from reality?** Update `sessions.md`. The plan is meant to evolve.
- **A session revealed a new concept that needs its own session?** Add it to `sessions.md`.
- **A milestone got split or merged in `docs/implementation/`?** Mirror the change here.

The plan is a living document. The Obsidian log is append-only history.

## Switching out of teaching mode

If at some point you decide you've learned enough Rust and want to hand the rest of the project to a coding agent for fast shipping:

1. Tell the agent: "drop teaching mode for the remaining milestones"
2. The agent reads `docs/implementation/tasks/` directly (which is clean, milestone-level)
3. This directory becomes archive — leave it for reference but don't update

The pristine tasks/ directory means the handover is friction-free.
