---
title: "lazydap doctor"
description: "Check that everything lazydap needs is where it should be"
---

:::note[Generated page]
From `lazydap doctor --help`. To change it, change the clap definition in [`crates/daemon/src/cli.rs`](https://github.com/planetaryescape/lazydap/blob/main/crates/daemon/src/cli.rs), then run `npm run generate` in `site/` and commit the result. CI fails if this page and the binary disagree.
:::

Check that everything lazydap needs is where it should be

```text
lazydap doctor [OPTIONS]
```

## Options

| Flag | Description |
| --- | --- |
| `--check-adapters` | Only check the adapters |
| `--check-state` | Only check the project state file |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
