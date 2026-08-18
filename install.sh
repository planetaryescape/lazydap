#!/usr/bin/env bash
#
# Install lazydap from a GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/planetaryescape/lazydap/main/install.sh | bash
#   ./install.sh v0.1.0          # a specific release rather than the newest
#
# Environment:
#   LAZYDAP_INSTALL_DIR   where the binary goes (default ~/.local/bin, created if absent)
#   LAZYDAP_REPO          owner/name to install from
#   LAZYDAP_BASE_URL      where release assets live. Must be https:// or file://;
#                         an absolute path is accepted and read as file://<path>
#   LAZYDAP_RELEASES_URL  where the release list comes from, same rules. Only read
#                         when no version argument is given
#   GITHUB_TOKEN, GH_TOKEN  optional. Raises the release-lookup rate limit from
#                         GitHub's anonymous 60 requests an hour. Sent to
#                         api.github.com and nowhere else, never printed, and
#                         retried without it if GitHub rejects it
#
# No sudo, ever. The only thing written outside a temporary directory is the one
# binary in LAZYDAP_INSTALL_DIR.

set -euo pipefail

die() {
  echo "install.sh: $*" >&2
  exit 1
}

require() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is required and was not found"
}

# An absolute path is a convenience spelling of file://<path>; curl needs the URL.
normalise_url() {
  case "$1" in
    /*) printf 'file://%s' "$1" ;;
    *) printf '%s' "$1" ;;
  esac
}

# Only two schemes. Plain http would let anyone between here and the origin serve
# both the archive and the digest that vouches for it, which is not a check at
# all; anything else curl still speaks has no business delivering a binary that
# is about to be run.
require_safe_scheme() {
  case "$1" in
    https://* | file://*) ;;
    *) die "refusing to fetch $1 — must be https:// or file:// (or an absolute path)" ;;
  esac
}

# A token is for the release *lookup* only. The assets come from a different
# host (github.com, redirected to objects.githubusercontent.com) and a public
# release needs no authentication to download — sending credentials to an origin
# that did not ask for them is how they end up in somebody else's logs.
GITHUB_API_TOKEN="${GITHUB_TOKEN:-${GH_TOKEN:-}}"

REPO="${LAZYDAP_REPO:-planetaryescape/lazydap}"
INSTALL_DIR="${LAZYDAP_INSTALL_DIR:-$HOME/.local/bin}"
REQUESTED="${1:-latest}"

BASE_URL="$(normalise_url "${LAZYDAP_BASE_URL:-https://github.com/${REPO}/releases/download}")"
require_safe_scheme "$BASE_URL"

RELEASES_URL="$(normalise_url "${LAZYDAP_RELEASES_URL:-https://api.github.com/repos/${REPO}/releases?per_page=100}")"
require_safe_scheme "$RELEASES_URL"

require curl
require tar
require install

if command -v shasum >/dev/null 2>&1; then
  sha256_of() { shasum -a 256 "$1" | awk '{ print tolower($1) }'; }
elif command -v sha256sum >/dev/null 2>&1; then
  sha256_of() { sha256sum "$1" | awk '{ print tolower($1) }'; }
else
  die "neither shasum nor sha256sum is available; refusing to install unverified"
fi

# Read the one digest out of a `shasum`-style manifest, or fail saying why.
#
# Deliberately not `shasum -c`: that hands the manifest the choice of which file
# gets checked, so a manifest naming some other file passes while the archive is
# never looked at. Here the manifest supplies a digest and nothing else — this
# script decides what gets hashed.
#
# The same parse lives in scripts/render_homebrew_formula.sh. It is duplicated
# because this file has to work when curled on its own, with nothing to source.
digest_from_manifest() {
  local manifest="$1"
  local expected="$2"

  # Strip CR first so a manifest written on Windows cannot smuggle one into the
  # digest and fail the comparison for a reason nobody could see.
  tr -d '\r' < "$manifest" | awk -v expected="$expected" '
    /[^[:space:]]/ {
      entries++
      digest = $1
      name = $2
      sub(/^\*/, "", name)   # sha256sum marks binary mode with a leading *
      fields = NF
    }
    END {
      if (entries != 1) {
        printf "expected exactly one entry, found %d\n", entries + 0 > "/dev/stderr"
        exit 1
      }
      if (fields != 2) {
        printf "entry has %d fields, expected 2\n", fields + 0 > "/dev/stderr"
        exit 1
      }
      if (digest !~ /^[0-9a-fA-F]{64}$/) {
        printf "not a sha-256 digest: %s\n", digest > "/dev/stderr"
        exit 1
      }
      if (name != expected) {
        printf "vouches for %s, not %s\n", name, expected > "/dev/stderr"
        exit 1
      }
      print tolower(digest)
    }
  '
}

# Fetch the release list, authenticating only if it is GitHub's own API.
#
# The scheme-and-host test is the whole safeguard: LAZYDAP_RELEASES_URL can name
# any origin, and the token belongs to exactly one of them.
#
# The header arrives through a config file on stdin rather than as an argument
# because `ps` shows one process's arguments to every user on the machine, and a
# token is not a thing to put there.
#
# A token that fails is worse than no token: an expired GITHUB_TOKEN exported in
# somebody's shell profile would otherwise turn an install that has always
# worked anonymously into a 401. So the authenticated attempt is a try, not a
# commitment — it falls back to the anonymous request the script would have made
# anyway, and says on stderr that it did. The token itself is never printed.
fetch_releases() {
  if [ -n "$GITHUB_API_TOKEN" ]; then
    case "$1" in
      https://api.github.com/*)
        if printf 'header = "Authorization: Bearer %s"\n' "$GITHUB_API_TOKEN" |
          curl -fsSL -K - "$1"; then
          return
        fi
        echo "install.sh: GITHUB_TOKEN was rejected; retrying without it" >&2
        ;;
    esac
  fi
  curl -fsSL "$1"
}

# Which build --------------------------------------------------------------
#
# Release assets are named for the Rust target triple they were built on, so
# this maps uname's answer onto that rather than inventing a second vocabulary.

os="$(uname -s)"
arch="$(uname -m)"

case "${os}/${arch}" in
  Darwin/arm64 | Darwin/aarch64) target="aarch64-apple-darwin" ;;
  Darwin/x86_64) target="x86_64-apple-darwin" ;;
  Linux/x86_64 | Linux/amd64) target="x86_64-unknown-linux-gnu" ;;
  *) die "no release build for ${os} ${arch}. Build it instead: cargo install --path crates/daemon" ;;
esac

# Which version ------------------------------------------------------------

if [ "$REQUESTED" = "latest" ]; then
  # Deliberately not the `releases/latest` redirect. This repository also
  # publishes `chapter-*` releases for the book, and product releases below 1.0
  # go out as prereleases, so that redirect points at a book chapter far more
  # often than it points at lazydap. Read the release list and choose from it.
  releases="$(fetch_releases "$RELEASES_URL")" ||
    die "could not reach ${RELEASES_URL}. Name a version instead: install.sh v0.1.0"

  # Two different notions of "prerelease" here, treated differently on purpose.
  #
  # GitHub's prerelease *flag* is ignored: every v0.* release carries it
  # deliberately, because a 0.x release is not a stability promise. Honouring it
  # would leave `latest` finding nothing at all until v1.0.
  #
  # A semver prerelease *suffix* is skipped: v0.2.0-rc1 is a candidate, and
  # somebody who named no version wants the newest release meant for them. Tags
  # are `vX.Y.Z` or `vX.Y.Z-suffix`, so a hyphen is the whole test.
  #
  # grep -o emits one match per line in document order and awk reads to EOF, so
  # neither depends on how the API happens to whitespace its JSON today.
  version="$(printf '%s' "$releases" |
    grep -o '"tag_name": *"v[^"]*"' |
    awk -F'"' '$4 !~ /-/ && !found { found = 1; print $4 }')"

  [ -n "$version" ] || die "no released v* version found in ${REPO}. Name one instead: install.sh v0.1.0"
  version="${version#v}"
else
  version="${REQUESTED#v}"
fi

name="lazydap-${version}-${target}"
archive="${name}.tar.gz"
url="${BASE_URL}/v${version}/${archive}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading ${url}"
curl -fsSL "$url" -o "${tmp}/${archive}" ||
  die "could not download ${url} — is v${version} a released version?"
curl -fsSL "${url}.sha256" -o "${tmp}/${archive}.sha256" ||
  die "could not download ${url}.sha256"

# Verify, then extract -----------------------------------------------------
#
# The order is the point: a checksum checked after the fact tells you what you
# have already unpacked onto your disk.
#
# What this proves: the bytes that arrived are the bytes the digest describes.
# That catches a truncated download, a corrupted mirror, a proxy that rewrote
# something in flight, and an archive swapped for a different one.
#
# What it does not prove: authenticity. The archive and the digest come from the
# same origin, so whoever controls that origin can serve a matching pair and this
# check says yes. Requiring https keeps a network attacker out of that origin;
# closing the rest needs a signature over the release, which is recorded as a
# follow-up in the M21 task file rather than something this script can fake.

echo "Verifying ${archive}"
expected_digest="$(digest_from_manifest "${tmp}/${archive}.sha256" "$archive")" ||
  die "${archive}.sha256 is not a usable checksum manifest. Nothing has been extracted."
actual_digest="$(sha256_of "${tmp}/${archive}")"

if [ "$expected_digest" != "$actual_digest" ]; then
  die "checksum mismatch for ${archive}
    expected ${expected_digest}
    actual   ${actual_digest}
  Refusing to install. Nothing has been extracted."
fi

echo "  sha256 ${actual_digest} matches"

echo "Extracting ${archive}"
tar -xzf "${tmp}/${archive}" -C "$tmp"

mkdir -p "$INSTALL_DIR"
install "${tmp}/${name}/lazydap" "${INSTALL_DIR}/lazydap"

echo
echo "Installed lazydap ${version} to ${INSTALL_DIR}/lazydap"
echo "  Check it: ${INSTALL_DIR}/lazydap version"
echo

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo "  ${INSTALL_DIR} is not on your PATH yet. Add it:"
    echo "      export PATH=\"${INSTALL_DIR}:\$PATH\""
    echo
    ;;
esac

echo "  lazydap drives debug adapters and bundles none of them. Install the one for"
echo "  the language you are debugging:"
echo "      C, C++, Rust   codelldb   https://github.com/vadimcn/codelldb/releases"
echo "      Python         debugpy    python3 -m pip install debugpy"
echo "      Go             delve      go install github.com/go-delve/delve/cmd/dlv@latest"
echo
echo "  codelldb has to reach your PATH through a wrapper script — a symlink breaks"
echo "  its liblldb lookup and it dies in dlopen. The four commands are in the README:"
echo "      https://github.com/${REPO}#install"
echo
echo "  Then run: lazydap doctor. One usable adapter is enough for it to pass."
