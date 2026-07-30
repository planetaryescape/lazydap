#!/usr/bin/env bash
#
# Enforce the crate dependency rules in ARCHITECTURE.md.
#
# The dependency graph *is* the architecture (D005): a client cannot bypass the
# IPC contract if it cannot depend on the daemon. Cargo enforces that only if
# somebody keeps the manifests honest, which is this script's job.
#
# Run from the workspace root. Adding a crate means adding it here.

set -euo pipefail

python3 - <<'PY'
from pathlib import Path
import sys
import tomllib

ROOT = Path.cwd()

# Which internal crates each crate is allowed to depend on. An empty set means
# "nothing internal at all" — the strongest boundary we have.
ALLOW = {
    # The floor of the graph: domain types, zero I/O, no internal deps.
    "lazydap-core": set(),
    # Raw DAP framing. Knows nothing about lazydap's own protocol, on purpose:
    # it is the thing adapters are written against.
    "lazydap-dap": set(),
    # The IPC contract. Depends on core so that any client — TUI, web, agent —
    # can speak it without pulling in the daemon.
    "lazydap-protocol": {"lazydap-core"},
    # Paths and (later) config loading.
    "lazydap-config": {"lazydap-core"},
    # Per-project state on disk (.lazydap/state.toml). Knows domain types and a
    # file format, and nothing about sockets, adapters or the daemon — so a
    # future client can read the same state without going through the daemon.
    "lazydap-store": {"lazydap-core"},
    # The TUI is a client and nothing more (D037). This is the row that makes
    # non-negotiable 2 structural rather than aspirational: with no path to the
    # daemon, the store or DAP, a TUI-only feature is not something that can be
    # written here at all — it has to become a protocol request, and a protocol
    # request is something the CLI can send too.
    "lazydap-tui": {"lazydap-config", "lazydap-core", "lazydap-protocol"},
    # The daemon may depend on everything, including the TUI — it is also the
    # `lazydap` binary, and a binary that could not start the TUI would need a
    # second one (D002 says there is one). The arrow only points this way:
    # daemon → tui is composition, tui → daemon would be the bypass, and the
    # row above is what forbids it.
    #
    # DAP is allowed here only because the adapter module is the seam that
    # keeps it from leaking further (see crates/daemon/src/adapter/mod.rs).
    "lazydap-daemon": {
        "lazydap-config",
        "lazydap-core",
        "lazydap-dap",
        "lazydap-protocol",
        "lazydap-store",
        "lazydap-tui",
    },
}

manifests = sorted((ROOT / "crates").glob("*/Cargo.toml"))
if not manifests:
    print("No crates found — run this from the workspace root.", file=sys.stderr)
    sys.exit(1)

errors = []
seen = set()

for manifest in manifests:
    data = tomllib.loads(manifest.read_text())
    package = data.get("package", {}).get("name")
    if package is None:
        continue

    rel = manifest.relative_to(ROOT)
    if package not in ALLOW:
        errors.append(
            f"{rel}: {package} is not in the dependency table. "
            "Add it to scripts/check_architecture_boundaries.sh and ARCHITECTURE.md."
        )
        continue

    seen.add(package)
    # Dev-dependencies are deliberately not checked: a test may reach for
    # anything, and the boundary being protected is what ships.
    deps = {
        name
        for name in data.get("dependencies", {})
        if name.startswith("lazydap-")
    }
    disallowed = sorted(deps - ALLOW[package])
    if disallowed:
        errors.append(
            f"{rel}: {package} has disallowed internal deps: {', '.join(disallowed)}"
        )

for package in sorted(set(ALLOW) - seen):
    errors.append(f"{package} is in the dependency table but not in the workspace.")

if errors:
    print("Architecture boundary violations:", file=sys.stderr)
    for error in errors:
        print(f"  - {error}", file=sys.stderr)
    sys.exit(1)

print(f"Architecture boundaries ok ({len(seen)} crates)")
PY
