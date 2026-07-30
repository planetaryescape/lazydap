---
title: "lazydap daemon"
description: "Run the daemon. Normally started automatically by the first command that needs it"
---

:::note[Generated page]
From `lazydap daemon --help`. To change it, change the clap definition in [`crates/daemon/src/cli.rs`](https://github.com/planetaryescape/lazydap/blob/main/crates/daemon/src/cli.rs), then run `npm run generate` in `site/` and commit the result. CI fails if this page and the binary disagree.
:::

Run the daemon. Normally started automatically by the first command that needs it

```text
lazydap daemon [OPTIONS]
```

## Options

| Flag | Description |
| --- | --- |
| `--foreground` | Stay in the terminal and log to stderr, for debugging |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
