---
title: "lazydap continue"
description: "Resume the program"
---

:::note[Generated page]
From `lazydap continue --help`. To change it, change the clap definition in [`crates/daemon/src/cli/`](https://github.com/planetaryescape/lazydap/tree/main/crates/daemon/src/cli) — the site rebuilds from the binary.
:::

Also spelled `lazydap c`.

Resume the program

```text
lazydap continue [OPTIONS]
```

## Options

| Flag | Description |
| --- | --- |
| `--wait` | Block until the program pauses, exits or is terminated, and return one JSON object describing everything that happened. Always use this from a script or an agent |
| `--timeout <TIMEOUT>` | How long to wait, in seconds. `0` waits forever. Defaults to 30, or to LAZYDAP_TIMEOUT |
| `--all-threads` | Wait for every thread to stop, not just the first |
| `--thread <THREAD>` | Which thread to resume. Defaults to the one that stopped last |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
