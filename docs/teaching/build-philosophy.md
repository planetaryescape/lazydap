# Build philosophy for tour-mode chapters

This book runs in **tour mode**: the learner doesn't rebuild the anchor codebase, they read it. So "the artifact at chapter close" works differently than in a build-from-zero book.

## What "artifact" means here

In a reproduce-mode book, each chapter ends with a runnable thing. In a tour-mode book, each chapter ends with **understanding the learner can demonstrate**:

- They can read a piece of the codebase they couldn't read at the start of the chapter.
- They can predict what a related piece of code probably does, before they've read it.
- They can explain the subsystem to a junior engineer in one paragraph.
- They can answer "why is this designed this way and not that way?" with the codebase's actual reasoning.

The artifact is a *paragraph*, not a binary. That's still demonstrable.

## The chapter-close ritual (tour-adapted)

**Open every chapter** by stating the artifact:
> "Today's concept is X. By the end of this chapter you'll be able to explain how Y works in this codebase, predict what the next file you've never read does, and justify the design choice."

**Close every chapter** by exercising the artifact:
- Re-read a key function. Notice the learner now understands it.
- Open a sibling file the learner hasn't read. Have them predict what's in it. Open it. Calibrate.
- Verbally explain the chapter's subsystem to an imaginary junior engineer.

**Make the ladder visible** at chapter close:
> "Last chapter you understood X. Now you understand X plus Y. Next chapter, X+Y enables you to follow Z."

The progression is real even though no code gets built. The chapter sequence builds *one mental model*, in layers.

## What if the learner wants to write code along the way?

Tour mode doesn't forbid it. Many chapters benefit from running the project locally, setting a breakpoint, and watching what actually happens. If a chapter motivates the learner to fork the project and add a tiny feature, do it — that's exactly the "you understand it now" demonstration tour mode is reaching for. Just don't *require* shipping code per chapter; that's not the contract.

## When this rule and the slowness rule (rule 12) seem to conflict

They don't. Slowness governs *pace within a chapter*; this rule governs *what the learner has to show at the end*. A slow chapter can produce a sharp paragraph of understanding. A fast chapter that produces fuzziness has violated both rules.

## Failure mode to watch for

If the learner finishes three chapters in a row and can't articulate what each chapter taught them, the chapters were skim-reads, not tours. Slow down. Re-read with predicts active. The pedagogy depends on the learner doing the work; tour mode makes it easier to fake doing the work because there's no compiler to call the bluff.

## See also

- `~/.skills/teaching/SKILL.md` — rule 13 (artifact per chapter, tour-mode adapted) and rule 15 (chapter-as-curriculum)
- `~/.skills/teaching/references/operating-rules.md` — the long-form rules
- The reproduce-mode counterpart at `references/templates/reproduce/build-philosophy.md`
