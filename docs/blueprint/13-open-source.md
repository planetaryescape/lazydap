# 13 — Open source, license, contribution

## License

Dual-licensed under **MIT OR Apache-2.0**. This is the Rust ecosystem convention, matches mxr, and maximises downstream usability.

Files at repo root:

- `LICENSE-MIT` — MIT License text
- `LICENSE-APACHE` — Apache License 2.0 text
- `README.md` mentions "Dual-licensed under MIT OR Apache-2.0"

Crate-level Cargo.toml entries:

```toml
[package]
license = "MIT OR Apache-2.0"
```

## Repository

`github.com/planetaryescape/lazydap` (proposed; matches mxr's home org).

Repo layout per [`/ARCHITECTURE.md`](../../ARCHITECTURE.md):

```
lazydap/
├── README.md
├── ARCHITECTURE.md
├── AGENTS.md
├── CLAUDE.md
├── TODO.md
├── LICENSE-MIT
├── LICENSE-APACHE
├── CONTRIBUTING.md          (ships with v0.1)
├── CODE_OF_CONDUCT.md       (ships with v0.1)
├── CHANGELOG.md             (release-please managed, ships with v0.1)
├── SECURITY.md              (ships with v0.1)
├── PRIVACY.md               (ships with v0.1)
├── Cargo.toml               (workspace)
├── rust-toolchain.toml      (pinned stable)
├── rustfmt.toml
├── clippy.toml
├── deny.toml                (cargo-deny config)
├── .github/
│   └── workflows/
│       ├── ci.yml
│       └── release.yml
├── crates/
├── docs/
├── examples/
├── tests/
├── benches/
└── lazydap.skill            (ZIP)
```

## Contribution rules

(Will live in `CONTRIBUTING.md` at v0.1; preview here.)

### Mandatory

1. **No feature lands without a CLI subcommand.** TUI-only PRs rejected.
2. **No feature lands without `--dry-run` if it mutates state.**
3. **Tests with real adapters, not mocks** (FakeAdapter exists for unit-style speed; integration tests run real codelldb).
4. **`cargo clippy --workspace --all-targets` must pass.**
5. **`cargo fmt --check` must pass.**
6. **JSON schema changes require a `15-decision-log.md` entry.**
7. **Crate boundary violations rejected at review.** Cargo.toml dependencies are the architecture.

### Strongly encouraged

1. **One PR per logical change.** Don't bundle "fix typo" with "rewrite session module."
2. **Conventional commits**: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`, `ci:`, `perf:`, `build:`. Used by release-please for changelog.
3. **Keep diffs small.** Big PRs are slow to review and hard to revert.
4. **No `unsafe` without an `// SAFETY:` comment.**
5. **Update relevant blueprint MDs when architecture changes.**

### Process

1. Open an issue describing the change before opening a PR (for non-trivial changes).
2. Fork, branch (`feat/your-feature`), implement.
3. Open PR against `main`. CI must pass.
4. Maintainer reviews; may request changes.
5. Squash-merge.

## Governance

v0.1 single maintainer. Post-v0.1 may evolve. Decisions about scope/direction in `15-decision-log.md`.

## Telemetry / privacy

(Lives in `PRIVACY.md` at v0.1; preview here.)

- **No telemetry.** Daemon doesn't phone home.
- **No analytics.** Crash reports, usage stats — none.
- **Logs** stored locally in `{data_dir}/lazydap.log`. Rotated, capped at 100MB, max 5 files.
- **State files** stored locally. Treat `.lazydap/state.toml` as sensitive (may contain expressions referencing internal APIs).
- **No update checks.** `cargo install lazydap --force` to upgrade.

## Security

(Lives in `SECURITY.md` at v0.1; preview here.)

- Vulnerabilities reported to `<contact>` (TODO: add).
- Daemon runs as the user, no privilege escalation.
- Adapter processes inherit user privileges (they need them to debug).
- Unix socket has user-only permissions (`0700`).
- No remote access. lazydap is local-first.

## Sponsor / fund

Not seeking funding pre-v0.1. Post-v0.1 may add GitHub Sponsors.

## See also

- [`14-roadmap.md`](14-roadmap.md) — when each piece ships
- [`15-decision-log.md`](15-decision-log.md) — D016 (license), D017 (repo)
