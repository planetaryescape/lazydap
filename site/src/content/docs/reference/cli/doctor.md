---
title: "lazydap doctor"
description: "Check that everything lazydap needs is where it should be"
---

:::note[Generated page]
From `lazydap doctor --help`. To change it, change the clap definition in [`crates/daemon/src/cli/`](https://github.com/planetaryescape/lazydap/tree/main/crates/daemon/src/cli) — the site rebuilds from the binary.
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
