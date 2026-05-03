# Teaching notes template

Each book chapter has a companion **teaching-notes file** at `docs/teaching/notes/<NN>-<title>.md`. This is the *teacher's* working file, not the public reader's. It accumulates refinements over multiple live sessions and is the first thing an agent should read before running the chapter live.

## Frontmatter

```yaml
---
chapter: 4
session_id: M0-1
title: Hello, adapter
sessions_run:
  - date: 2026-05-02
    learner: <name or pseudonym>
    duration_minutes: 75
    notes_below: true
---
```

## Body structure

```markdown
# Teaching notes — Chapter NN: <Title>

## Concept anchor

(One paragraph: the **one** new concept this chapter teaches. State it precisely. Everything else in the chapter is in service of this. If a session expanded the scope past one concept, that's a flag — refactor the chapter and update the session plan.)

## Common wrong predictions

For each predict-pause in the chapter, what real learners answer wrong, why they go there, and how the chapter calibrates.

| Predict question | Common wrong answer | Why learners go there | How the chapter currently calibrates |
|---|---|---|---|
| `<question>` | `<answer>` | `<root cause — usually a JS/TS/Python prior model>` | `<the discussion in the <details> block>` |

## What surprised the learner

(Things the learner found unexpected that the chapter could pre-empt. If 3+ learners are surprised by the same thing, lift it from "surprise" to "explicit explanation" in the chapter.)

## Sticky points (concepts that needed a second pass)

(Sections of the chapter where the learner asked clarifying questions, paused longer than expected, or where the teach-back was rough. These are candidates for chapter revision.)

## Refinement ideas

- [ ] (idea) — (rationale) — (when to apply: next session / next refresh / wait for N more data points)

## Notes for future sessions on this chapter

(Things to do *differently* next time you teach this. Examples: "open with a specific anchor"; "skip section X if the learner already nailed it elsewhere"; "spend extra time on Y because two learners in a row stalled there".)

## Did the artifact land?

(Per session: did the learner end the session with the runnable artifact working? If no, what blocked? Rule 13 violation, or environment issue, or genuine learning gap?)

## Reuse log

(When this chapter has been used to teach this concept *outside* the project's main session sequence — e.g., a refresher for a returning learner — note it. Helps spot when the chapter has earned promotion to a "stable" status vs needing more refinement.)
```

## When to update

- **After every live session** that uses the chapter — even if no new wrong predictions came up, log it under "sessions run" and "did the artifact land".
- **Before re-teaching the chapter** — read the existing notes; pre-empt past sticky points.
- **When a refinement idea has 3+ data points** — lift it from "ideas" into a chapter revision.

## What NOT to put here

- The learner's *full* predictions verbatim — anonymise to the *pattern*, not the specific person. The chapter readership over time will grow; protect privacy.
- Anything that should be in the chapter itself — if a clarification helps the public reader, edit the chapter. The teaching notes are for *teacher-only* meta.
- Critique of the learner — write for the *next teacher*, not as a personal assessment. The frame is always "what could the chapter do better."
