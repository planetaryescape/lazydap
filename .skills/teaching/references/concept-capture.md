# Concept capture — writing atomic notes during teaching sessions

When a session introduces a concept that's worth long-term retention, create or extend an atomic note in the learner's Obsidian vault. This is how teaching compounds — over months, the vault becomes a personal reference of *what they actually learned*, in their own context, with the analogies and breaks that mattered to them.

## When to create a new atomic note

Create when:

- A new concept is introduced for the first time
- The concept has substantive content (not a one-line aside)
- The concept will recur in future sessions or future code
- The concept doesn't already have a note (search first via the Obsidian skill)

Don't create when:

- It's a passing reference to a concept that already has a note (just link)
- The "concept" is really a syntax detail (those go in inline code, not concept notes)
- The concept is too granular ("`String::new()` returns an empty String") — that's documentation, not knowledge

## When to extend an existing note

Extend when:

- The session deepened understanding of an existing concept
- The session revealed a new edge case or anti-pattern
- The session connected the concept to something new (in which case, also add a `related:` link)
- The session showed an idiom or example that should live in the note for future reference

## Atomic concept note template

```markdown
---
tags:
  - resources
  - resources/programming/<language>
  - resources/programming/<language>/<sub-topic>      # if applicable
type: "[[Resource Note]]"
date: <YYYY-MM-DD>
related:
  - "[[<Other concept>]]"
  - "[[<Project where it was learned>]]"
  - "[[<Session note where it was first introduced>]]"
---

# <Concept>

<One paragraph: what this is, in plain language. Lead with the *what* and the *why*. Save *how* for the body.>

## The mental model

<How to think about it. Visual analogies, mental pictures. The *intuition* — not the spec.>

### The JS/TS analog (and where it breaks)

<For Rust learners with JS/TS background. The bridge — and explicitly where it stops working.>

## In code

<Minimal example showing the concept. Real code from the project where possible (lazydap, mxr, etc.). Cite the source.>

```rust
// Concrete example.
```

## Common gotchas

<Things that bit the learner during the session. The compiler errors that revealed the concept. The "obvious" mistakes that aren't actually obvious.>

## Why it exists

<The motivation. What problem does this solve? What would go wrong without it? The "why" matters as much as the "what" — adult learners need this.>

## See also

- [[<Related concept>]] — <one-line on the relationship>
- [[<Sister concept>]] — <relationship>
- [[<Project>]] — <where it was learned in context>
- [[<Session note>]] — <when it was first introduced>

## Further reading

- <Official docs link if relevant>
- <Blog post or video that explained it well>
```

## Naming conventions

```
<Domain> <Concept>.md
```

Examples:
- `Rust Ownership.md`
- `Rust Lifetimes.md`
- `Rust Traits vs TypeScript Interfaces.md` (when the analogy break is the key insight)
- `Rust Async Send Bound.md` (specific subtopic)

Match the language and tone of existing vault notes (Title Case, no abbreviations unless universally known, sentence-case headings inside).

## Linking — apply the obsidian skill's linking protocol

Every concept note created or extended triggers the linking protocol:

1. Grep the vault for related keywords; suggest links
2. Tag-search siblings under the same hierarchical tag
3. Bidirectional pass: if A relates to B, consider adding A to B's `related`
4. Cluster check: if 3+ atomic notes circle a sub-topic without a synthesis, propose one (e.g., 3 notes about strings → `Rust Strings.md` synthesis)

The Obsidian skill (`~/.dotfiles/.agents/skills/obsidian/SKILL.md`) encodes all of this. Always invoke it for vault writes; don't hand-roll.

## When the concept doesn't fit cleanly

Some concepts are gradient — they don't have a clean "atomic" boundary. Examples:

- "The Rust async ecosystem"
- "Error handling in Rust"
- "Concurrency in Rust"

These are **synthesis-level**, not atomic. Don't try to write a single atomic note for them. Instead:

- Wait for atomic notes to accumulate around the topic (`Async fn`, `tokio::spawn`, `Send bound`, `Pin`...)
- When 3+ accumulate, propose the synthesis note that ties them together
- The synthesis note links to the atoms; the atoms each get a `related:` link back

This is the Obsidian emergent-synthesis principle. Apply it.

## When the learner is the one writing the concept note

Sometimes the most valuable note is one **the learner writes themselves** as the teach-back. Two ways to do this:

1. **At session end**, ask the learner to write the concept note from scratch in their own words. You then review and suggest tweaks.
2. **You write a draft**, then ask the learner to rewrite the "mental model" section in their own words. The hybrid version is the kept one.

The learner-written note is more valuable than a polished AI-written one because:

- The vocabulary matches how *they* think
- Six months from now, *their* phrasing will be more recallable than yours
- The act of writing is part of the learning

When in doubt: have the learner write the note. Slower, better.
