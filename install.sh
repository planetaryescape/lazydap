#!/usr/bin/env bash
#
# Install lazydap from a GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/planetaryescape/lazydap/main/install.sh | bash
#   ./install.sh v0.1.0          # a specific release rather than the newest
#
# Environment:
#   LAZYDAP_INSTALL_DIR  where the binary goes (default ~/.local/bin, created if absent)
#   LAZYDAP_REPO         owner/name to install from
#   LAZYDAP_BASE_URL     where release assets live; point it at a mirror, or at a
#                        local directory to rehearse this script offline
#
# No sudo, ever. The only thing written outside a temporary directory is the one
# binary in LAZYDAP_INSTALL_DIR.

set -euo pipefail

REPO="${LAZYDAP_REPO:-planetaryescape/lazydap}"
BASE_URL="${LAZYDAP_BASE_URL:-https://github.com/${REPO}/releases/download}"
INSTALL_DIR="${LAZYDAP_INSTALL_DIR:-$HOME/.local/bin}"
REQUESTED="${1:-latest}"

die() {
  echo "install.sh: $*" >&2
  exit 1
}

require() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is required and was not found"
}

require curl
require tar
require install

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
  # often than it points at lazydap. Read the release list and take the newest
  # `v*` tag instead.
  releases="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases?per_page=100")" ||
    die "could not reach the GitHub API. Name a version instead: install.sh v0.1.0"

  # grep -o emits one match per line in document order, and awk reads to EOF,
  # so neither depends on how the API happens to whitespace its JSON today.
  version="$(printf '%s' "$releases" |
    grep -o '"tag_name": *"v[^"]*"' |
    awk -F'"' 'NR == 1 { print $4 }')"

  [ -n "$version" ] || die "no v* release found in ${REPO}. Name a version instead: install.sh v0.1.0"
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
# In that order, and the order is the point: a checksum checked after the fact
# tells you what you have already unpacked onto your disk.

echo "Verifying ${archive}"
if command -v shasum >/dev/null 2>&1; then
  (cd "$tmp" && shasum -a 256 -c "${archive}.sha256")
elif command -v sha256sum >/dev/null 2>&1; then
  (cd "$tmp" && sha256sum -c "${archive}.sha256")
else
  die "neither shasum nor sha256sum is available; refusing to install unverified"
fi

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

echo "  lazydap drives codelldb and does not bundle it. codelldb has to reach your"
echo "  PATH through a wrapper script — a symlink breaks its liblldb lookup and it"
echo "  dies in dlopen. The four commands are in the README:"
echo "      https://github.com/${REPO}#install"
echo
echo "  Then run: lazydap doctor"
