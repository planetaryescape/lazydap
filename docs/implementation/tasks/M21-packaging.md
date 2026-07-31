# M21 — Packaging: install.sh and Homebrew

## What

Two install channels the release audit found missing (2026-07-31): a curl-able `install.sh`
at the repo root, and a Homebrew formula in a `planetaryescape/homebrew-lazydap` tap,
rendered and pushed by the release workflow. mxr ships both; its implementation is the
template.

## Why

`brew install` and `curl | sh` are how most people actually install a CLI. The release
tarball instructions work but ask for five manual steps; the ship-it checklist in AGENTS.md
explicitly records these channels as not-yet-existing so nothing claims them — this
milestone makes the claims true.

## How

Reference implementation: `~/code/planetaryescape/mxr` — `install.sh`,
`packaging/homebrew/mxr.rb`, `scripts/render_homebrew_formula.sh`, and the `homebrew` job
in its `.github/workflows/release.yml` (secret: `HOMEBREW_TAP_TOKEN`).

1. **`install.sh`** at repo root: OS/arch detection (macOS arm64/x86_64, Linux x86_64),
   version argument defaulting to latest (resolved via the releases/latest redirect),
   download tarball + `.sha256`, verify BEFORE extracting, install to
   `LAZYDAP_INSTALL_DIR` (default `~/.local/bin`, created if absent), print PATH guidance
   and the codelldb reminder. `set -euo pipefail`, no sudo ever. Asset names follow this
   repo's convention: `lazydap-{version}-{target}.tar.gz` (note: differs from mxr's naming).
2. **Formula template** `packaging/homebrew/lazydap.rb`: binary formula, three targets,
   `__VERSION__`/`__SHA256_*__` placeholders, installs `lazydap` + licenses; caveats
   section pointing at the codelldb wrapper-script install (quirks 1/5).
3. **Render script** `scripts/render_homebrew_formula.sh`: version + artifact dir in,
   formula out; fails loudly on a missing checksum.
4. **Release workflow**: a `homebrew` job after publish — render, clone tap with
   `HOMEBREW_TAP_TOKEN`, commit, push. Skipped with a clear log line when the secret is
   absent (forks, rehearsals).
5. **Docs**: README install section gains both channels (brew first), site install page
   likewise, release-notes template in the workflow gains the one-liners, AGENTS.md
   ship-it §6 verification list gains both channels and drops the "do not claim" line.
6. **Verification without a new release**: render the formula against the existing v0.1.0
   assets (download, checksum) and `brew install --formula ./lazydap.rb` locally;
   `MXR-style install.sh` test: `LAZYDAP_INSTALL_DIR="$(mktemp -d)" ./install.sh v0.1.0`
   then run `version`. Both must report 0.1.0.

## Success criteria

- `LAZYDAP_INSTALL_DIR="$(mktemp -d)" ./install.sh` (latest) and `./install.sh v0.1.0`
  both produce a working binary; checksum tampering fails before extraction.
- `brew install --formula packaging/homebrew/lazydap-rendered.rb` installs a working
  binary locally (tap push verified at next release).
- Workflow YAML actionlint-clean; the homebrew job no-ops gracefully without the secret.
- README/site claims match reality: nothing promises the tap before it exists.

## Files

- `install.sh` (new, repo root)
- `packaging/homebrew/lazydap.rb` (new)
- `scripts/render_homebrew_formula.sh` (new)
- `.github/workflows/product-release.yml` (homebrew job)
- `README.md`, `site/src/content/docs/getting-started/install.md`, `AGENTS.md` §ship-it

## Verify

```bash
LAZYDAP_INSTALL_DIR="$(mktemp -d)" ./install.sh v0.1.0
bash scripts/render_homebrew_formula.sh 0.1.0 <artifact-dir> /tmp/lazydap.rb
brew install --formula /tmp/lazydap.rb && lazydap version && brew uninstall lazydap
```

## Depends on

- v0.1.0 release assets (exist). Outward-facing halves need the user:
  `planetaryescape/homebrew-lazydap` repo creation and a `HOMEBREW_TAP_TOKEN` secret
  (PAT with write access to the tap) — orchestrator handles with user auth.

## Notes

- Created 2026-07-31 after the fresh-install audit; user: "fix the gaps".
- crates.io stays out (D051). The tap is per-project (`homebrew-lazydap`), matching mxr's
  `homebrew-mxr` rather than a shared tap.
