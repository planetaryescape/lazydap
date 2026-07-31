#!/usr/bin/env bash
#
# Fill the Homebrew formula template with a version and the three checksums the
# release workflow computed on the runners that built each tarball.
#
# Run from the workspace root, like the other scripts here.
#
#   bash scripts/render_homebrew_formula.sh 0.1.0 dist Formula/lazydap.rb
#
# A missing checksum is fatal rather than a placeholder left in the output: a
# formula carrying `__SHA256_...__` would install nothing and say so only at the
# point somebody tried to `brew install` it.

set -euo pipefail

if [ $# -ne 3 ]; then
  echo "usage: $0 <version> <artifacts-dir> <output-formula>" >&2
  exit 1
fi

version="${1#v}"
artifacts_dir="$2"
output_formula="$3"
template="packaging/homebrew/lazydap.rb"

[ -f "$template" ] || {
  echo "$0: no $template — run this from the workspace root" >&2
  exit 1
}

checksum_for() {
  local target="$1"
  local archive="lazydap-${version}-${target}.tar.gz"
  local file="${artifacts_dir}/${archive}.sha256"

  if [ ! -f "$file" ]; then
    echo "$0: missing checksum file: $file" >&2
    return 1
  fi

  # The same parse as install.sh's digest_from_manifest, and for the same
  # reason. A checksum file that is empty, doubled, CRLF-wrapped or vouching for
  # a different archive would otherwise be substituted into the formula as-is:
  # `sha256 ""` renders fine, publishes fine, and fails on somebody else's
  # machine at `brew install`, which is the worst place to find out.
  tr -d '\r' < "$file" | awk -v expected="$archive" -v file="$file" '
    /[^[:space:]]/ {
      entries++
      digest = $1
      name = $2
      sub(/^\*/, "", name)
      fields = NF
    }
    END {
      if (entries != 1) {
        printf "%s: expected exactly one entry, found %d\n", file, entries + 0 > "/dev/stderr"
        exit 1
      }
      if (fields != 2) {
        printf "%s: entry has %d fields, expected 2\n", file, fields + 0 > "/dev/stderr"
        exit 1
      }
      if (digest !~ /^[0-9a-fA-F]{64}$/) {
        printf "%s: not a sha-256 digest: %s\n", file, digest > "/dev/stderr"
        exit 1
      }
      if (name != expected) {
        printf "%s: vouches for %s, not %s\n", file, name, expected > "/dev/stderr"
        exit 1
      }
      print tolower(digest)
    }
  '
}

# `|| exit 1` on each: the failure happens inside a command substitution, which
# is its own subshell, so the function's own exit would not stop this script.
aarch64_apple_darwin="$(checksum_for aarch64-apple-darwin)" || exit 1
x86_64_apple_darwin="$(checksum_for x86_64-apple-darwin)" || exit 1
x86_64_unknown_linux_gnu="$(checksum_for x86_64-unknown-linux-gnu)" || exit 1

mkdir -p "$(dirname "$output_formula")"

sed \
  -e "s/__VERSION__/${version}/g" \
  -e "s/__SHA256_AARCH64_APPLE_DARWIN__/${aarch64_apple_darwin}/g" \
  -e "s/__SHA256_X86_64_APPLE_DARWIN__/${x86_64_apple_darwin}/g" \
  -e "s/__SHA256_X86_64_UNKNOWN_LINUX_GNU__/${x86_64_unknown_linux_gnu}/g" \
  "$template" > "$output_formula"

echo "$0: wrote $output_formula for v${version}"
