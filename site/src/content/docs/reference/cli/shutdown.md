---
title: "lazydap shutdown"
description: "Stop the daemon and every session it owns"
---

:::note[Generated page]
From `lazydap shutdown --help`. To change it, change the clap definition in [`crates/daemon/src/cli.rs`](https://github.com/planetaryescape/lazydap/blob/main/crates/daemon/src/cli.rs), then run `npm run generate` in `site/` and commit the result. CI fails if this page and the binary disagree.
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
