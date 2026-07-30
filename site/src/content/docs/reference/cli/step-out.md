---
title: "lazydap step-out"
description: "Run until the current function returns"
---

:::note[Generated page]
From `lazydap step-out --help`. To change it, change the clap definition in [`crates/daemon/src/cli.rs`](https://github.com/planetaryescape/lazydap/blob/main/crates/daemon/src/cli.rs), then run `npm run generate` in `site/` and commit the result. CI fails if this page and the binary disagree.
:::

Run until the current function returns

```text
lazydap step-out [OPTIONS]
```

## Options

| Flag | Description |
| --- | --- |
| `--wait` | Block until the program pauses, exits or is terminated, and return one JSON object describing everything that happened. Always use this from a script or an agent |
| `--timeout <TIMEOUT>` | How long to wait, in seconds. `0` waits forever. Defaults to 30, or to LAZYDAP_TIMEOUT |
| `--thread <THREAD>` | — |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
