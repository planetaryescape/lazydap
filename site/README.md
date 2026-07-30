# lazydap docs site

Astro + Starlight. Sources in `src/content/docs/`, built to `dist/`.

## Build it

```bash
npm ci
npm run build
```

That needs Node and nothing else. **No Rust toolchain, no `lazydap` binary** — which is the
point: a clean clone builds, and so does a Vercel deploy, where cargo does not exist.

`build` runs `validate` → `astro build` → `generate-llms-txt`.

## The CLI reference is generated, and committed

`src/content/docs/reference/cli/` is produced by `scripts/generate-cli-reference.mjs`, which
runs `lazydap --help` and one `lazydap <command> --help` per command. Nothing on those pages
is transcribed by hand.

Those pages are **committed** so that a flag change shows up in a review diff, and so the
site builds without a binary. That only works if somebody keeps them current:

```bash
cargo build --bin lazydap          # from the repository root
cd site && npm run generate        # rewrites reference/cli/
git add src/content/docs/reference/cli
```

Do that whenever you change a clap definition in `crates/daemon/src/cli.rs`.

CI does it for you in reverse: the `site` job builds the binary, regenerates, and fails if
git sees a diff. So forgetting is caught, not shipped.

```bash
npm run check:generated            # what CI runs; exits 1 on drift
```

The generator finds the binary at `$LAZYDAP_BIN`, or else whichever of
`target/release/lazydap` and `target/debug/lazydap` was built most recently. CI sets
`LAZYDAP_BIN` explicitly so a stale binary cannot make the check pass by accident.

## Wire-format examples are generated too

The JSON frames in `reference/protocol.md` are pasted from a program that serialises the real
types, because hand-written serde shapes get subtly wrong (a unit variant is `"Ping"`, not
`{"Ping":null}`, and a client built from the wrong shape is answered `BadRequest`):

```bash
cargo run -p lazydap-protocol --example wire_examples
```

Re-run it and update the page whenever `crates/protocol/src/types.rs` changes.

## Validation

```bash
npm run validate
```

Checks internal links resolve, slugs are unique, frontmatter is complete, and that a handful
of claims the blueprint docs get wrong cannot be copied in. Add a rule to
`scripts/validate-docs.mjs` when you find a mistake worth never making twice.

## Writing

One [Diataxis](https://diataxis.fr/) mode per page. Every command shown must have been run
against the current binary, with real output pasted and elisions marked. Positioning claims
must pass the differentiation test: if a competitor could not credibly claim the opposite, it
is not a claim worth making.

## Deployment

Not set up. `vercel.json` is present and correct; no project exists and no domain is chosen.
`astro.config.mjs` reads `SITE_URL` from the environment and falls back to a placeholder.
