#!/usr/bin/env bash
#
# Build `lazydap.skill`, the agent skill ZIP (D027).
#
# Two jobs: regenerate `skill/references/commands.md` from lazydap's own
# argument parser, and pack `skill/` into the ZIP at the repository root.
#
# The output is **byte-for-byte reproducible**. A ZIP records a modification
# time per entry, so an ordinary `zip -r` produces a different file every run,
# and a committed artifact that changes on every build is one nobody can review
# and everybody re-commits by accident. Every entry is stamped with a fixed
# date and added in sorted order, so rebuilding without changing a source file
# produces an identical ZIP and `git diff` stays quiet.
#
# Run from the workspace root.

set -euo pipefail

SOURCE_DIR="skill"
OUTPUT="lazydap.skill"
# Arbitrary, fixed, and in the past. The value does not matter; that it never
# changes does.
STAMP="200001010000"

if [[ ! -d "$SOURCE_DIR" ]]; then
    echo "no $SOURCE_DIR/ directory — run this from the workspace root" >&2
    exit 1
fi

for tool in zip cargo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "$tool is needed to build the skill" >&2
        exit 1
    fi
done

echo "generating $SOURCE_DIR/references/commands.md"
cargo run --quiet --example gen_skill_commands -- "$SOURCE_DIR/references/commands.md"

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT

cp -R "$SOURCE_DIR/." "$staging/"

# Same mtime on every entry, or the ZIP differs run to run.
find "$staging" -exec touch -t "$STAMP" {} +

# `-X` drops the extra file attributes (uid, gid, host timestamps) that would
# otherwise vary by machine. The explicit sorted file list fixes entry order,
# which `zip -r` does not guarantee across filesystems.
(
    cd "$staging"
    find . -type f | LC_ALL=C sort | zip -q -X "$OLDPWD/$OUTPUT.tmp" -@
)

mv "$OUTPUT.tmp" "$OUTPUT"
echo "wrote $OUTPUT"
unzip -l "$OUTPUT" | tail -n +2
