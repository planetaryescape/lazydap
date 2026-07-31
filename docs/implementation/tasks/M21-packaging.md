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

**2026-07-31.** Done and verified against the real v0.1.0 release assets.
`planetaryescape/homebrew-lazydap` exists and `HOMEBREW_TAP_TOKEN` is set, so the workflow
has everything it needs. The one thing left is pushing the rendered v0.1.0 formula into the
tap by hand — v0.1.0 shipped before this milestone existed, so no workflow run has ever
updated the tap. Every version from here does it automatically.

Shipped:

- `install.sh` — `LAZYDAP_INSTALL_DIR` (default `~/.local/bin`), `LAZYDAP_REPO`,
  `LAZYDAP_BASE_URL`, `LAZYDAP_RELEASES_URL`, optional version argument, no sudo,
  shellcheck-clean.
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

### Security review round (2026-07-31)

Seven findings, all fixed in this milestone.

- **The checksum check had no trust anchor and did not bind the digest to the file.**
  `shasum -c` lets the *manifest* choose what gets checked, so a manifest naming a file that
  trivially matches passes while the archive is never hashed. `install.sh` now parses the
  manifest itself — exactly one entry, exactly two fields, a 64-hex digest, and a filename
  that must equal the archive it is about to install — hashes the downloaded file directly,
  and string-compares. `shasum -c` is never handed the manifest.
- **Download schemes are restricted to `https://` and `file://`.** Over plain http the same
  attacker serves the archive and the digest that vouches for it, which is not a check.
- **What the check does and does not prove is written down** at the point it happens:
  integrity against a corrupted or swapped download, not authenticity. The archive and its
  digest share an origin, so a compromised origin defeats it.
- **Tap updates were racy.** The workflow's concurrency group is per-ref, so two releases in
  flight could interleave and the loser could quietly restore the older formula. The
  `homebrew` job now has a global concurrency group and refuses (logs, succeeds) when the
  version in the tap is newer than the one being pushed, compared with `sort -V`.
- **The release notes advertised Homebrew unconditionally** while the tap update could skip
  or fail. The brew line is now appended by the tap job after the push succeeds, so a
  release whose tap update skipped never mentions brew at all.
- **Malformed checksum files rendered silently** into the formula — an empty file produced
  `sha256 ""`, which publishes fine and fails at somebody else's `brew install`. The render
  script applies the same parse as `install.sh`.
- **`latest` would have selected a future `v0.2.0-rc1` over `v0.1.0`.** Settled policy: the
  GitHub prerelease *flag* is ignored, because every `v0.*` release carries it deliberately
  and honouring it would leave `latest` finding nothing until v1.0; a semver prerelease
  *suffix* is skipped.

### Follow-up: sign the releases

Checksums prove integrity, not authenticity — an attacker who controls the release origin
serves a matching archive and digest and every check here passes. Closing that needs a
signature over the release artifacts (minisign or cosign), a published public key, and
verification in both `install.sh` and the formula. Out of M21's scope; worth its own
milestone before anyone treats lazydap as a supply-chain dependency.
