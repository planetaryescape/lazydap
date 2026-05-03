# 0001 — CONTRIBUTING.md's codelldb symlink install is broken

**Status:** open
**Discovered:** 2026-05-02 (session M0-1)
**Affects:** [`CONTRIBUTING.md`](../../CONTRIBUTING.md) — codelldb section
**Priority:** medium (blocks M0+ for new contributors who follow CONTRIBUTING.md literally)

## Summary

The current CONTRIBUTING.md instructs readers to expose codelldb on PATH via:

```bash
ln -sf ~/.local/opt/codelldb/extension/adapter/codelldb ~/.local/bin/codelldb
```

This **does not work**. When invoked via the symlink, codelldb panics on startup with:

```
called Result::unwrap() on an Err value:
"dlopen(/Users/<user>/.local/lldb/lib/liblldb.dylib, ...) (no such file)"
```

## Root cause

codelldb computes the location of `liblldb.dylib` at runtime by taking `argv[0]`, stripping the basename, and appending `../lldb/lib/liblldb.dylib`. When invoked through a symlink at `~/.local/bin/codelldb`, the computed path resolves to `~/.local/lldb/lib/liblldb.dylib`, which doesn't exist. The actual `liblldb.dylib` is at `~/.local/opt/codelldb/extension/lldb/lib/liblldb.dylib`.

This is a baked-in behaviour of the codelldb binary. Verified via `otool -L` showing no static or rpath dependency on liblldb (it's `dlopen`'d at runtime via path computation).

## Fix

Replace the `ln -sf` step with a wrapper-script install, the same pattern the js-debug section already uses. The wrapper invokes the real binary with an absolute path, so `argv[0]` resolves correctly relative to the bundle.

```bash
cat > ~/.local/bin/codelldb <<'WRAPPER_EOF'
#!/usr/bin/env bash
exec "$HOME/.local/opt/codelldb/extension/adapter/codelldb" "$@"
WRAPPER_EOF
chmod +x ~/.local/bin/codelldb
```

This pattern is used in CONTRIBUTING.md's existing **js-debug** section. Codifying it for codelldb just makes the project's three adapter installs uniform.

## Cross-references

- [`docs/reference/codelldb-quirks.md`](../reference/codelldb-quirks.md): full forensic write-up of the dynamic-library lookup behaviour
- [`docs/book/04-hello-adapter.md`](../book/04-hello-adapter.md): chapter that hits this issue and points at this fix
- Issue [0002](0002-codelldb-version-drift-rust-log.md): related but distinct codelldb version-drift problem

## Resolution criteria

- [ ] CONTRIBUTING.md's codelldb section uses the wrapper-script pattern (resolved by [date])
- [ ] An existing learner with the symlink install can `which codelldb` resolve to the wrapper, and `codelldb --help` runs cleanly
- [ ] `cargo run --example m0_hello_adapter` succeeds for a fresh contributor following CONTRIBUTING.md verbatim
