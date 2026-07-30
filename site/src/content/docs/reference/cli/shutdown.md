---
title: "lazydap shutdown"
description: "Stop the daemon and every session it owns"
---

:::note[Generated page]
From `lazydap shutdown --help`. To change it, change the clap definition in [`crates/daemon/src/cli/`](https://github.com/planetaryescape/lazydap/tree/main/crates/daemon/src/cli) — the site rebuilds from the binary.
:::

Stop the daemon and every session it owns

```text
lazydap shutdown [OPTIONS]
```

## Options

| Flag | Description |
| --- | --- |
| `--dry-run` | Report what would be stopped, and stop nothing |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
