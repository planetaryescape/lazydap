---
title: "lazydap step-in"
description: "Step into the call on this line"
---

:::note[Generated page]
From `lazydap step-in --help`. To change it, change the clap definition in [`crates/daemon/src/cli/`](https://github.com/planetaryescape/lazydap/tree/main/crates/daemon/src/cli) — the site rebuilds from the binary.
:::

Also spelled `lazydap step-into`.

Step into the call on this line

```text
lazydap step-in [OPTIONS]
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
