---
title: "lazydap disconnect"
description: "End the current session"
---

:::note[Generated page]
From `lazydap disconnect --help`. To change it, change the clap definition in [`crates/daemon/src/cli.rs`](https://github.com/planetaryescape/lazydap/blob/main/crates/daemon/src/cli.rs), then run `npm run generate` in `site/` and commit the result. CI fails if this page and the binary disagree.
:::

End the current session

```text
lazydap disconnect [OPTIONS]
```

## Options

| Flag | Description |
| --- | --- |
| `--session-id <SESSION_ID>` | Which session to end. Defaults to the active one |
| `--no-terminate` | Leave the debuggee running instead of killing it |
| `--dry-run` | Report what would be ended, and end nothing |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
