---
title: "lazydap output"
description: "Show output the debuggee has produced"
---

:::note[Generated page]
From `lazydap output --help`. To change it, change the clap definition in [`crates/daemon/src/cli/`](https://github.com/planetaryescape/lazydap/tree/main/crates/daemon/src/cli) — the site rebuilds from the binary.
:::

Show output the debuggee has produced

```text
lazydap output [OPTIONS]
```

## Options

| Flag | Description |
| --- | --- |
| `--since <SINCE>` | Only output at or after this Unix-epoch millisecond |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
