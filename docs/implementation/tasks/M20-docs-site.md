# M20 — Documentation website

## What

A public documentation site for lazydap at `site/`: Astro + Starlight, mirroring the shape
that works for mxr (`~/code/planetaryescape/mxr/site` is the reference implementation).
Getting-started guides, task guides, generated CLI reference, JSON schema reference,
troubleshooting — everything a new user or a new agent needs, on a website.

## Why

The README carries the front door, but a README cannot hold forty pages. mxr's site is the
proven pattern: Starlight sidebar with Start Here / Guides / Reference, a CLI reference
generated from the real command tree so it cannot drift, `llms.txt` for agent consumption,
and a validation script that keeps links honest. lazydap sells to agents and their humans;
both read docs sites.

## How

1. `site/` scaffold: Astro + Starlight pinned to current versions (mxr uses
   `@astrojs/starlight` 0.41.x / `astro` 7.x — check current), custom CSS, og/social meta,
   sidebar in `astro.config.mjs`.
2. Content, sourced from what exists (the repo is the source of truth — docs/articles for
   positioning, skill/references for agent-facing reference, docs/reference for quirks,
   README for the quickstart — rewritten for the medium, not pasted):
   - **Start Here**: install (source build now, binaries at v0.1), codelldb setup (quirks 1/5
     wrapper-script install), quickstart (the verified loop), TUI tour.
   - **Guides**: debugging with an agent (the skill, `--wait` discipline), breakpoints
     (persistence, conditions, dry-run), the `--wait` contract, output formats and piping,
     the daemon (instances, auto-spawn, logs, doctor), why-lazydap (positioning, the five
     trade-offs).
   - **Reference**: CLI (generated — adapt mxr's `generate-cli-reference.mjs` to walk
     `lazydap <cmd> --help`, or reuse the `gen_skill_commands` example output), JSON output
     schemas, error codes + exit codes, codelldb quirks, protocol (for client authors),
     architecture.
   - **Troubleshooting**: the quirks doc reshaped as symptoms → fixes.
3. Build pipeline in `site/package.json` mirroring mxr: `generate` → `validate` → `astro
   build` → `llms.txt` postbuild. Port mxr's `validate-docs.mjs` approach (dead links,
   generated-files freshness).
4. `vercel.json` parity with mxr. Deployment itself is NOT part of this milestone — the
   domain and Vercel project are the user's call; the site must simply `npm run build`
   green from a clean clone.
5. CI: a `site` job that runs the build on PRs touching `site/` (npm ci + build; keep it
   out of the Rust gate path).

## Success criteria

- `cd site && npm install && npm run build` green from scratch; zero dead internal links.
- CLI reference page regenerates from the binary and a freshness check fails the build when
  it drifts (same discipline as the skill ZIP).
- Every command shown on any page runs verbatim against the current binary (verify-before-
  publishing; transcripts real, elisions marked).
- The positioning pages pass the differentiation test (no claim a competitor couldn't
  credibly invert).
- `llms.txt` emitted; docs readable without JavaScript.
- Honest pre-release banner until v0.1 tags.

## Files

- `site/**` (new — scaffold, content, scripts)
- `.github/workflows/ci.yml` — one `site` job
- `scripts/` — only if a generator needs to live outside `site/scripts/`

## Verify

```bash
cd site && npm ci && npm run build
```

## Depends on

- M6/M7 (CLI surface + skill are the content), W5b's README (front-door copy alignment).
  The M15 config code half is NOT a dependency — the site documents what ships today and
  gains a config page at Wave 6.

## Notes

- Created 2026-07-30 on user request mid-ship-mode: "just like mxr, lazydap will need a
  website with all the documentation and getting started guides".
- The `writing-docs` skill governs the prose; load it before writing any page.
- Deployment/domain decision deliberately excluded — surfaced to the user at completion.
