---
title: "lazydap eval"
description: "Evaluate an expression in the debuggee"
---

:::note[Generated page]
From `lazydap eval --help`. To change it, change the clap definition in [`crates/daemon/src/cli.rs`](https://github.com/planetaryescape/lazydap/blob/main/crates/daemon/src/cli.rs), then run `npm run generate` in `site/` and commit the result. CI fails if this page and the binary disagree.
:::

Evaluate an expression in the debuggee

```text
lazydap eval [OPTIONS] <EXPRESSION>
```

## Arguments

| Argument | Description |
| --- | --- |
| `<EXPRESSION>` | The expression, in the debuggee's own language |

## Options

| Flag | Description |
| --- | --- |
| `--frame <FRAME>` | Which frame to evaluate in. Defaults to the top one |
| `--context <CONTEXT>` | How the adapter should read the expression. `watch` and `hover` evaluate it in the program; `repl` runs it as an adapter command, which for codelldb means an LLDB command [default: watch] |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
