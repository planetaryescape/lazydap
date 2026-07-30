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

## Completion note

**Completed 2026-07-30.** `cd site && npm ci && npm run build` is green from a clean tree,
run twice, 39 pages each time with no diff between runs. Zero validation errors, zero dead
internal links. The four Rust gates and the boundary script were re-run afterwards and are
green; no Rust was touched.

### Pages

17 hand-written pages plus 22 generated ones.

| Path | Purpose |
|---|---|
| `index.mdx` | Splash landing: the loop, what it is, the five trade-offs in brief |
| `getting-started/install.md` | codelldb via wrapper script, lazydap from source, `doctor` |
| `getting-started/quickstart.md` | Breakpoint → variable → exit against a real C program |
| `getting-started/tui.md` | Keys, tty detection, why the TUI is a client |
| `guides/why-lazydap.md` | Positioning: the five trade-offs, named alternatives, falsifiers |
| `guides/agents.md` | The skill bundle, `--wait` discipline, mistakes that cost turns |
| `guides/wait.md` | Stable states, the five outcomes, timeouts, coalescing, queueing |
| `guides/breakpoints.md` | Persistence, `verified`, conditions, hit counts, log points, dry-run |
| `guides/output-formats.md` | The five formats, piping, `jq` recipes, errors on stderr |
| `guides/daemon.md` | Per-project instances, auto-spawn, paths, logs, version mismatch |
| `guides/architecture.md` | Three stacked protocols, the 7-crate graph, four IPC buckets |
| `reference/cli/index.md` | Generated: command list plus the options every command takes |
| `reference/cli/<cmd>.md` | Generated: one page per top-level command (22) |
| `reference/json-output.md` | Field-by-field schemas, all captured from real runs |
| `reference/errors.md` | Exit codes 0–4, the error object, every error name |
| `reference/protocol.md` | Socket wire format for client authors |
| `reference/codelldb-quirks.md` | The eight quirks, cause and fix each |
| `troubleshooting.md` | The same failures organised by symptom |

### Build pipeline

`npm run build` = `generate` → `validate` → `astro build` → `generate-llms-txt`.

- `scripts/generate-cli-reference.mjs` shells out to the built binary and walks
  `lazydap --help` → `lazydap <cmd> --help`. Nothing is transcribed by hand. Options common
  to every command are hoisted to the index rather than repeated 22 times. Binary resolution
  is `$LAZYDAP_BIN`, then `target/release`, then `target/debug`.
- `scripts/validate-docs.mjs` checks internal links resolve, slugs are unique, frontmatter is
  complete, and that specific stale claims cannot be copied in from the blueprint —
  CamelCase wire values, the eleven-crate list, the "sixth IPC bucket" line, `xargs -r`, and
  banned marketing vocabulary. It caught one real mistake during authoring.
- `scripts/generate-llms-txt.mjs` writes `llms-full.txt` and a `.md` sibling per page. The
  curated `llms.txt` is hand-written at `public/llms.txt`.
- `npm run check:generated` regenerates and `git diff --exit-code`s. Verified to exit 1 on a
  committed-but-stale reference page, and 0 otherwise.

### Deviations from the plan

1. **Spelling.** The brief said `docs/` uses American technical spelling. It does not — the
   repo is consistently British (`behaviour` 37, `serialis*` 31, `normalis*` 8). Pages follow
   the repo.
2. **Dependency pins.** `astro` 7.1.3 / `@astrojs/starlight` 0.41.4, not the current 7.1.6 /
   0.41.5. The npm registry in this environment refuses anything published after 2026-07-23.
   Bumping is a one-line change plus a lockfile refresh, and worth doing on a machine without
   the cutoff.
3. **`site/.npmrc`.** npm here runs with `--strict-allow-scripts`, so `esbuild` and
   `fsevents` are allowlisted per-project. Harmless on a default-configured CI runner and
   keeps `npm ci` non-interactive.
4. **The CI job is not path-gated.** The brief asked for it to run on `site/**` changes. It
   runs unconditionally instead: the CLI reference is generated from the clap definitions in
   `crates/daemon`, so a Rust-only PR renaming a flag is exactly what makes the committed
   reference stale, and a path filter would let that through. It is a separate job, so the
   Rust gates never wait on it.
5. **No `sharp`.** mxr needs it for social-image generation; nothing here generates images.
6. **`public/og.png` does not exist.** The `og:image` meta tags are wired and point at it, so
   dropping a 1200×630 PNG at that path is the whole remaining task. Until then social cards
   degrade to no image. Flagged with a TODO in `astro.config.mjs`.
7. **`TODO.md` not ticked.** Another worker owns that file in this wave; the orchestrator
   should add the M20 row when merging.

### Verified transcripts

Every command shown was run against `target/debug/lazydap` at this commit, with codelldb
1.12.2 on Darwin 25.5.0, against a C program built with `gcc -g -O0`. Home directories are
rewritten to `/Users/you`; elisions are marked in the prose. Covered: `doctor`, `break`
(add / list / conditional / dry-run remove / remove), `launch --stop-on-entry`,
`continue --wait` to a breakpoint, to a timeout, and to exit, `step --wait`, `pause --wait`,
`stack`, `scopes`, `variables`, `eval` (success and failure), `threads`, `output`, `status`,
`version`, `logs`, `disconnect`, the `table`/`json`/`jsonl`/`csv`/`ids` formats, and the
`SessionNotPaused`, `SessionNotFound` and `UsageError` failures.

Two findings worth keeping: `--timeout 3` against a program printing once a second returns
`"state": "timeout"` with all three lines still in `captured_output`, which is the evidence
for "output survives a timeout"; and a conditional breakpoint `i == 7` stopped with
`sum == 21`, which is arithmetically the right iteration.

### Left open for the user

**Deployment and domain.** No Vercel project was created and nothing was deployed, per the
milestone. `site/vercel.json` matches mxr's shape (`buildCommand`, `outputDirectory`,
`framework`) minus mxr's apex-redirect rule, which is domain-specific. `astro.config.mjs`
reads `SITE_URL` from the environment and falls back to `https://lazydap.sh`, a placeholder
chosen only for symmetry with `mxr.sh` — it appears in canonical tags, the sitemap,
`robots.txt` and `llms-full.txt`, so it needs deciding before the first deploy rather than
after.
