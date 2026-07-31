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

## Completed

**2026-07-31.** Everything in this repository is done and verified against the real v0.1.0
release assets. The two outward-facing halves are not, and cannot be from here: the
`planetaryescape/homebrew-lazydap` repository does not exist and `HOMEBREW_TAP_TOKEN` is not
set. Until both land, `brew install planetaryescape/lazydap/lazydap` fails, which is why the
README and the site both carry a note saying the tap arrives with the next release.

Shipped:

- `install.sh` — `LAZYDAP_INSTALL_DIR` (default `~/.local/bin`), `LAZYDAP_REPO`,
  `LAZYDAP_BASE_URL`, optional version argument, no sudo, shellcheck-clean.
- `packaging/homebrew/lazydap.rb` and `scripts/render_homebrew_formula.sh`.
- A `homebrew` job in `.github/workflows/product-release.yml`, after `publish`.
- README, `site/src/content/docs/getting-started/install.md`, AGENTS.md ship-it §6, and the
  release-notes template in the workflow.
- D060 in the decision log.

### Deviations from the plan

- **"Resolved via the releases/latest redirect" does not work here** and the plan said to do
  it because mxr does. That redirect answered `chapter-08` on the day this was written: the
  repository publishes book-chapter releases from the other workflow, and 0.x product
  releases are prereleases, which the redirect skips on principle. `install.sh` reads the
  release list and takes the newest `v*` tag instead. D060.
- **`LAZYDAP_BASE_URL` was not in the plan.** It exists so the download host can be pointed
  at a mirror, and so the checksum-before-extract behaviour can be rehearsed end to end
  against a deliberately corrupted tarball, which is otherwise untestable without publishing
  a bad release.
- **The formula's licence field** is `license any_of: ["MIT", "Apache-2.0"]`, not the
  `"MIT OR Apache-2.0"` string mxr's formula uses. `brew audit` rejects the string form as a
  non-standard SPDX licence.
- **`prefix.install "README.md"` is not in the formula** even though the plan said to install
  the docs. Homebrew copies `README.md` and `CHANGELOG.md` out of the tarball on its own; the
  two `LICENSE-*` files are the ones it does not recognise, so those are explicit. Verified
  by listing the installed keg.
- **The plan's `brew install --formula /tmp/lazydap.rb` no longer works.** Homebrew 6.x
  refuses formulae outside a tap. Verification used `brew tap-new` to make a throwaway local
  tap, installed from that, and untapped afterwards.

### Follow-ups discovered

- `brew uninstall` now runs `brew autoremove` on its way out, and during verification it
  removed two unrelated orphaned formulae from the machine. Release verification should use
  `HOMEBREW_NO_AUTOREMOVE=1`; AGENTS.md §6 says so.
- The site install page's sample `doctor` output still says `protocol v2`; the daemon is at
  v5. Untouched here — out of this milestone's blast radius.
- No Linux arm64 release build exists, so `install.sh` sends aarch64 Linux to a source
  build. Worth a target if anyone asks.
