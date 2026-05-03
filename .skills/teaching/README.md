# /teaching skill

Pair-programming pedagogy for teaching a senior engineer something new — a language, a paradigm, a library, a domain — while building a real project. **Project-agnostic.** Works on any codebase, in any language.

## When invoked

- User explicitly says "teach me", "let's go slow", "I want to understand X", "I want to learn", "show me how", "walk me through"
- Working on any project whose `AGENTS.md` declares teaching mode (e.g., lazydap is the first; future projects can opt in the same way)
- User asks for a session-style learning interaction

## What it does

- Caps cognitive load (one new concept per session)
- Surfaces the learner's existing mental model before teaching anything new
- Uses prediction-before-execution as the diagnostic
- Ladders responsibility from "I do" → "we do" → "you do"
- Lets the compiler be a co-teacher (no pre-empting errors)
- Captures conceptual increments as atomic notes via the Obsidian skill

## Files

- `SKILL.md` — main entry point, the twelve operating rules, when to invoke
- `references/operating-rules.md` — expanded rules with concrete examples
- `references/pedagogy-frameworks.md` — PRIMM, GRR, CLT, etc. with citations
- `references/session-template.md` — Obsidian session note template
- `references/concept-capture.md` — how to write atomic concept notes

## Depends on

- The `obsidian` skill (`~/.dotfiles/.agents/skills/obsidian/`) for vault CRUD

## See also

- Vault hub: `Teaching Senior Engineers.md` (the pedagogy itself, mirrored from this skill)
- Per-project integration: each project that uses teaching mode adds a section to its own `AGENTS.md` referencing this skill. First example: `~/code/planetaryescape/lazydap/AGENTS.md`. Same pattern for future projects.

## Portability

Designed to be agent-portable. The pedagogy lives in this skill + AGENTS.md + Obsidian. Works in Claude Code, Codex, OpenCode, Cursor, or any agent that reads skill files and follows AGENTS.md conventions.
