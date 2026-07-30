---
title: Output formats and piping
description: Pick a format, pipe ids into the next command, and know which output is a contract.
---

Every command takes `--format`. Pick `json` when something will parse it, `table` when you
will read it, and `ids` when the output feeds the next command.

```bash
lazydap stack --format json | jq '.frames[0].line'
```

With no `--format`, lazydap chooses: `table` when stdout is a terminal, `json` everywhere
else. So a pipeline gets JSON without asking, and a human gets columns without asking.

## The five

### `json` — the contract

One object or array. The schema is stable and breaking it costs a decision-log entry.

```console
$ lazydap variables --reference 1003 --format json
{
  "variables": [
    { "name": "n",   "type_name": "int", "value": "10", "variables_reference": 0 },
    { "name": "sum", "type_name": "int", "value": "21", "variables_reference": 0 },
    { "name": "i",   "type_name": "int", "value": "7",  "variables_reference": 0 }
  ]
}
```

### `table` — not a contract

```console
$ lazydap variables --reference 1003 --format table
NAME  VALUE  TYPE  REFERENCE
n     10     int   0
sum   21     int   0
i     7      int   0
```

Column order and widths change whenever they read badly. Do not parse this. If you are
reaching for `awk` here, you want `--format json` or `--format csv`.

### `jsonl` — one object per line

For streams and `while read`. Arrays come out as one element per line rather than one blob:

```console
$ lazydap stack --format jsonl
{"column":16,"id":1010,"line":6,"name":"total","source":{"name":"hello.c","path":"/Users/you/lazydap-demo/hello.c"}}
{"column":18,"id":1011,"line":14,"name":"main","source":{"name":"hello.c","path":"/Users/you/lazydap-demo/hello.c"}}
{"column":0,"id":1012,"line":1751,"name":"start","source":{"name":"@start","source_reference":1000}}
```

### `csv` — for spreadsheets and `cut`

```console
$ lazydap stack --format csv
frame,name,source,line
1007,total,/Users/you/lazydap-demo/hello.c,6
1008,main,/Users/you/lazydap-demo/hello.c,14
1009,start,@start,1751
```

A flattened view, so nested fields are dropped rather than escaped. Use `json` when you need
all of it.

### `ids` — bare ids for `xargs`

```console
$ lazydap break --list --format ids
1
```

One id per line, nothing else. This is the format that makes commands compose.

## Piping between commands

`--id` is repeatable and reads exactly what `--format ids` writes:

```bash
lazydap break --list --format ids | xargs -I{} lazydap break --remove --id {}
```

Preview it first. Every mutation takes `--dry-run` and reports the same selection the real run
would act on:

```bash
lazydap break --remove --all --dry-run --format json
lazydap break --remove --all
```

`xargs -r` is GNU-only and does nothing on macOS. When an empty list is possible, guard it:

```bash
ids=$(lazydap break --list --format ids)
[ -n "$ids" ] && printf '%s\n' "$ids" | xargs -I{} lazydap break --remove --id {}
```

## Reading a `--wait` reply with `jq`

```bash
result=$(lazydap continue --wait --format json)

printf '%s' "$result" | jq -r .state
printf '%s' "$result" | jq -r '.frame | "\(.source.name):\(.line) in \(.name)"'
printf '%s' "$result" | jq -r '.captured_output[] | select(.category=="stdout") | .output'
```

The last one is the program's own output, separated from the adapter's `console` chatter.

## Errors

Errors go to **stderr** as JSON, and stdout stays empty. So a pipeline that parses stdout
does not get an error object where it expected data:

```console
$ lazydap variables --ref 1006 --format json
{"details":{"kind":"UnknownArgument"},"error":"UsageError","message":"error: unexpected argument '--ref' found\n\n  tip: a similar argument exists: '--reference'\n\nUsage: lazydap variables --reference <REFERENCE>\n\nFor more information, try '--help'."}
$ echo $?
2
```

The exit code is the signal to branch on. [Errors and exit codes](/reference/errors/) lists
all of them.

## See also

- [JSON output](/reference/json-output/) — field-by-field schemas
- [`lazydap break`](/reference/cli/break/) — selection flags in full
- [Debug with an agent](/guides/agents/) — the same formats, from a model
