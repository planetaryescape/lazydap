# 0003 — Chapter stub drift

**Date:** 2026-05-16
**Scope:** `docs/book/08-*.md` – `docs/book/39-*.md` (stubs only; chapters 01–07 checked for status drift only)
**Sources:** `docs/teaching/sessions.md`, `docs/implementation/tasks/M*.md`, `docs/book/README.md`

---

## Summary

Six categories of drift found. No broken milestone file paths. No orphaned stubs (all stub session IDs exist in sessions.md). Main issues: one session in sessions.md (TDD-1) has no stub and skews the Phase B session count; chapters 05–07 are taught but lack `status: complete`; README TOC has 11 title mismatches and 2 missing links; all stubs omit prev/next navigation fields.

---

## Concept drift

Minor wording only — `and` vs `+` in chapter titles. No substantive concept mismatch found for chapters 09–39 against their sessions.md rows.

Two cases worth attention:

| Ch | Stub title | sessions.md concept/what | Delta |
|---|---|---|---|
| 23 | "Source pane and scrolling" | "Source pane + scrolling" | Also called "Show a file" in README row 23 — three different names for the same chapter |
| 29 | "Stack pane" | "Stack pane + frame nav" | Stub drops the "frame nav" qualifier from sessions.md |
| 31 | "Lazy expand variables" | "Lazy expand on `<CR>`" | Stub loses the keybinding callout |

---

## Session-plan drift

**TDD-1 is listed in sessions.md (Phase B / M5 block) but has no corresponding stub chapter.**

sessions.md M5 block lists six entries: M5-1, **TDD-1**, M5-2, M5-3, M5-4, M5-5. The Phase B count table says 10 sessions, and 10 stubs exist (chapters 12–21). TDD-1 is not counted in the table and has no stub. Either:

- TDD-1 is intentionally a live-only meta-session with no chapter (count is right, but then the M5 row is misleading), or
- TDD-1 should become a stub between ch12 and ch13 (which would push Phase B to 11 sessions and the total to 40).

**Decision needed: does TDD-1 get its own chapter?**

---

## Milestone-doc drift

All `related_milestone` paths in stub frontmatter resolve to existing files. No broken paths. No artifact mismatch found beyond minor wording.

---

## Cross-link drift

No stub chapter has `prev_chapter` or `next_chapter` fields in its YAML frontmatter. Navigation links are completely absent from all 32 stubs. May be intentional (fill during live session), but worth deciding explicitly so the README's "Picking up mid-book" section stays credible.

---

## TOC drift

### Missing links in README (ch06, ch07)

Chapters 06 and 07 have files (`06-serde-typed-protocols.md`, `07-dap-transport-and-seq.md`) but their README TOC rows are unlinked plain text. Sessions.md marks both as "✅ taught".

### Title mismatches: README TOC vs stub frontmatter

| Ch | README TOC title | Stub `title` field |
|---|---|---|
| 06 | Serde + typed protocols | Serde and typed protocols |
| 07 | The transport struct + atomic seq | DAP transport and atomic seq |
| 08 | Event streaming + tagged enums | Event streaming and tagged enums |
| 12 | The protocol crate | The protocol crate and IpcMessage envelope |
| 14 | Unix sockets + accept loop | Unix sockets and the accept loop |
| 18 | The `--wait` design | The wait design |
| 21 | Skill + agent verification | Skill ZIP and agent verification |
| 23 | Show a file | Source pane and scrolling |
| 33 | Config crate | Config crate and global config |
| 36 | Watches pane + persist | Watches pane and persistence |
| 39 | Adapter routing | Adapter routing and auto-detect |

Ch23 is the worst: README and frontmatter use entirely different names.

### Chapters 05–07: taught but no `status` field

Sessions.md marks M1-1 (ch05), M2-1 (ch06), M2-2 (ch07) as "✅ taught". None of these three chapters has a `status:` field in frontmatter — not `stub`, not `complete`. They are effectively in an unknown state.

---

## New sessions in sessions.md without matching stub

| Session | Description | Expected chapter |
|---|---|---|
| TDD-1 | Test-driven development meta-session (retroactive explanation) | None exists; would sit between ch12 and ch13 |

---

## Removed/renamed sessions still stubbed

None. All 32 stub `session_id` values (M3-1 through M18-2) appear in sessions.md. No orphans.

---

## Recommended triage order

1. **Decide TDD-1 fate** — chapter or no chapter? Update the count table in sessions.md accordingly. Highest-impact ambiguity.
2. **Add `status: complete` to ch05–07** — quick, removes the unknown-state gap.
3. **Fix README links for ch06 and ch07** — two-line edit, currently these chapters are unreachable from the TOC.
4. **Reconcile ch23 name** — "Show a file" vs "Source pane and scrolling" is the biggest title inconsistency. Pick one and apply to both README and frontmatter.
5. **Audit remaining 10 README title mismatches** — all `+` vs `and` style; decide canonical form and apply consistently.
6. **Decide prev/next link policy** — document the decision in AGENTS.md or sessions.md so it doesn't look like accidental omission.

---

## Self-test

- Stubs checked: chapters 08–39 (32 files). Chapters 01–07 checked for status only.
- Milestone files checked: M00–M18 (18 files). All exist.
- README TOC rows checked: all 39.
- Session IDs cross-referenced against sessions.md table: all checked.
- `prev_chapter`/`next_chapter` grep: returned empty for all 32 stubs.
- No auto-fixes applied. Read-only sweep.
