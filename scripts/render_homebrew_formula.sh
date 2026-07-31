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
  local file="${artifacts_dir}/lazydap-${version}-${target}.tar.gz.sha256"

  if [ ! -f "$file" ]; then
    echo "$0: missing checksum file: $file" >&2
    exit 1
  fi

  awk '{ print $1 }' "$file"
}

aarch64_apple_darwin="$(checksum_for aarch64-apple-darwin)"
x86_64_apple_darwin="$(checksum_for x86_64-apple-darwin)"
x86_64_unknown_linux_gnu="$(checksum_for x86_64-unknown-linux-gnu)"

mkdir -p "$(dirname "$output_formula")"

sed \
  -e "s/__VERSION__/${version}/g" \
  -e "s/__SHA256_AARCH64_APPLE_DARWIN__/${aarch64_apple_darwin}/g" \
  -e "s/__SHA256_X86_64_APPLE_DARWIN__/${x86_64_apple_darwin}/g" \
  -e "s/__SHA256_X86_64_UNKNOWN_LINUX_GNU__/${x86_64_unknown_linux_gnu}/g" \
  "$template" > "$output_formula"

echo "$0: wrote $output_formula for v${version}"
