---
title: "lazydap variables"
description: "Expand a scope or a structured variable"
---

:::note[Generated page]
From `lazydap variables --help`. To change it, change the clap definition in [`crates/daemon/src/cli.rs`](https://github.com/planetaryescape/lazydap/blob/main/crates/daemon/src/cli.rs), then run `npm run generate` in `site/` and commit the result. CI fails if this page and the binary disagree.
:::

Expand a scope or a structured variable

```text
lazydap variables [OPTIONS] --reference <REFERENCE>
```

## Options

| Flag | Description |
| --- | --- |
| `--reference <REFERENCE>` | The `variables_reference` from `scopes` or a parent variable |
| `--filter <FILTER>` | Fetch only named members or only indexed elements [default: all] |
| `--start <START>` | Skip this many |
| `--count <COUNT>` | Take at most this many |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
