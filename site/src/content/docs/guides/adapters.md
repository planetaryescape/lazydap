---
title: Write one script for four languages
description: What differs between codelldb, debugpy and delve, and the seven rules that keep one code path working against all of them.
---

lazydap presents one CLI over three debug adapters — codelldb for C, C++ and Rust, debugpy for
Python, delve for Go. Most of the surface really is the same: `launch`, `break`,
`continue --wait`, `stack`, `scopes`, `variables`, `eval`, and the `state` field you branch on.

The differences are small in number and unevenly distributed in cost. A few are cosmetic. A
few will make a script that works perfectly against Python return confident wrong answers
against Go. This page is the list, and the rules that follow from it.

Everything here was verified on 2026-08-01 against codelldb 1.12.2, debugpy 1.8.21 on CPython
3.14.6, and delve 1.27.0 on Go 1.26.5, running C, Rust, Python and Go fixtures on macOS
(Darwin 25.5.0).

## What differs

| | codelldb (C, C++, Rust) | debugpy (Python) | delve (Go) |
|---|---|---|---|
| Scope names | `Local`, `Static`, `Global`, `Registers` | `Locals`, `Globals` | `Locals` |
| `type_name` on a variable | present | present | **absent — no key at all** |
| `hit_breakpoint_ids` at a breakpoint stop | populated | **always `[]`** | populated |
| Program stdout line ending | `\r\n` | `\n` | `\n` |
| `frame.column` | real | always `1` | always `0` |
| Breakpoint on a line with no statement | slides **forward** | slides **backward** | **refused**, with a message |
| Breakpoint past end of file | `verified: false` | **`verified: true`**, moved to the last line | `verified: false` |
| `--stop-on-entry` `reason` | `entry` | `entry` | `entry` |
| `raw_reason` at that stop | `exception` | `null` | `null` |
| `eval` can call functions | **no** | yes | yes |

Two rows carry most of the risk: `type_name`, which is a *shape* difference and so breaks code
rather than confusing it, and the breakpoint rows, where all three adapters answer the same
question differently and only one of them tells you.

## The rules

### 1. Read the locals scope positionally, not by name

The scope you want is the first one. It is called `Local` under codelldb and `Locals` under the
other two, so a name match on either spelling silently finds nothing against a third of your
languages.

```bash
lazydap scopes --format json | jq '.scopes[0].variables_reference'
```

Every adapter puts locals first. codelldb then adds `Static`, `Global` and `Registers`;
debugpy adds `Globals`; delve adds nothing:

```json
// codelldb
{ "scopes": [ { "expensive": false, "name": "Local",     "variables_reference": 1003 },
              { "expensive": false, "name": "Static",    "variables_reference": 1004 },
              { "expensive": false, "name": "Global",    "variables_reference": 1005 },
              { "expensive": false, "name": "Registers", "variables_reference": 1006 } ] }

// debugpy
{ "scopes": [ { "expensive": false, "name": "Locals",  "variables_reference": 3 },
              { "expensive": false, "name": "Globals", "variables_reference": 4 } ] }

// delve
{ "scopes": [ { "expensive": false, "name": "Locals", "variables_reference": 1000 } ] }
```

If you need globals, match case-insensitively on a `global` prefix and accept that delve has
none.

### 2. Do not require `type_name`

delve omits DAP's optional `type` field entirely and puts the type inside the value string.
Because lazydap omits absent optional fields rather than writing `null`, Go variables have **no
`type_name` key**:

```json
// delve
{ "name": "p", "value": "main.Pt {X: 1, Y: 2}", "variables_reference": 1003 }
{ "name": "s", "value": "[]int len: 3, cap: 3, [1,2,3]", "indexed_variables": 3, "variables_reference": 1004 }

// codelldb
{ "name": "v_int", "type_name": "alloc::vec::Vec<int, alloc::alloc::Global>", "value": "size=5", "variables_reference": 1014 }
```

`v["type_name"]` raises against Go where it returns a string against C and Python. Use
`v.get("type_name")` and have a path for `None`.

Do not branch on its *contents* either. codelldb reports Rust primitives under C names — `i8`
arrives as `char`, `i64` as `long`, `u64` and `usize` both as `unsigned long` — so the string
is not the declared type even when it is there.

### 3. Branch on `reason` and `frame.line`, not `hit_breakpoint_ids`

debugpy never populates it. A Python breakpoint stop is complete at:

```json
{ "frame": { "column": 1, "id": 2, "line": 16, "name": "<module>", "source": { "path": "/Users/you/pyq.py" } },
  "hit_breakpoint_ids": [],
  "reason": "breakpoint",
  "state": "paused" }
```

codelldb and delve both fill it in (`[4]` and `[8]` in the runs behind this page), so code
tested against those two looks correct until it meets Python — and then finds no breakpoint,
every time, with no error.

`reason` plus `frame.source.path` and `frame.line` identify the stop everywhere. The only thing
they cannot do is distinguish two breakpoints on the same line, which is a good reason not to
set two breakpoints on the same line.

### 4. Strip `\r` from captured output

codelldb sends `\r\n`, the other two send `\n`, for the same program printing the same string:

```json
{ "category": "stdout", "output": "hello from cq\r\n" }      // codelldb, C
{ "category": "stdout", "output": "{1 2} [1 2 3] 42 hi\n" }   // delve, Go
```

Any comparison against expected text needs the `\r` gone first.

While you are there: **concatenate chunks before splitting into lines.** A chunk is not a line.
debugpy splits a single `print("before")` into two output events, the text and its newline,
with identical timestamps:

```json
"captured_output": [
  { "category": "stdout", "output": "before", "timestamp_ms": 1785621783847 },
  { "category": "stdout", "output": "\n",     "timestamp_ms": 1785621783847 }
]
```

Splitting per chunk gives you a spurious blank line for every `print`.

### 5. Treat `column` as advisory

Only codelldb measures it. debugpy always sends `1`; delve always sends `0`, which is not even
a legal DAP column, since they are 1-based.

```json
{ "column": 1, "id": 3, "line": 3,  "name": "main",      "source": { "path": "/Users/you/pyq.py" } }  // debugpy
{ "column": 0, "id": 1000, "line": 15, "name": "main.main", "source": { "path": "/Users/you/goq.go" } }  // delve
```

Use it to render a caret if you like. Never use it to slice a string — delve's `0` will
off-by-one a 1-based index and debugpy's `1` will point at the first character of every line
forever.

### 6. Check `verified`, and compare `adapter_line` against `line`

This is the one that produces wrong answers rather than errors.

A breakpoint on a line with no statement gets three different treatments. Against a C file
whose line 2 is blank, codelldb moves it **forward** to the first statement, line 4:

```json
{ "adapter_line": 4, "enabled": true, "id": 9, "line": 2, "message": "Resolved locations: 0",
  "source": "/Users/you/cq.c", "verified": true }
```

Against a Python file whose line 6 is blank, debugpy moves it **backward** to line 5 — into the
block you were trying to step out of:

```json
{ "adapter_line": 5, "enabled": true, "id": 6, "line": 6, "source": "/Users/you/pyq.py", "verified": true }
```

delve refuses, and says why:

```json
{ "enabled": true, "id": 7, "line": 2,
  "message": "could not find statement at /Users/you/goq.go:2, please use a line with a statement",
  "source": "/Users/you/goq.go", "verified": false }
```

Worse, debugpy accepts a line past the end of the file. On a **16-line** file:

```json
{ "adapter_line": 16, "enabled": true, "id": 5, "line": 99999, "source": "/Users/you/pyq.py", "verified": true }
```

`verified: true`, no `message`, and the program really does stop at line 16. codelldb and delve
both report `verified: false` for the same input.

So `verified` alone is not enough. The portable check is both fields:

```bash
lazydap launch ./prog --format json \
  | jq '.breakpoints[] | select(.verified == false or (.adapter_line // .line) != .line)'
```

Anything that comes out of that either did not bind or is not where you asked. `adapter_line`
is present on every adapter when a breakpoint moves, which makes it the one reliable signal
across all three.

### 7. Expect `eval` to be the least portable thing you do

Function and method calls work under debugpy and delve and **not** under codelldb, which
rejects them at the open parenthesis:

```console
$ lazydap eval "v_int.len()" --format json      # codelldb
... "adapter_message":"Syntax error: v_int.len()\n                       ^"

$ lazydap eval "len([1,2,3])" --format json     # debugpy
{ "type_name": "int", "value": "3", "variables_reference": 0 }

$ lazydap eval "len(s)" --format json           # delve
{ "value": "3", "variables_reference": 0 }
```

Error text has nothing in common either. delve answers every failure with `Unable to evaluate
expression`, naming neither the identifier nor the cause. codelldb prefixes unknown-identifier
errors with a banner that reads like a catastrophe and is not one:

```
Expression evaluation in Rust not supported. Falling back to default language.
Ran expression as 'Objective C++'.
error: <user expression 0>:1:1: use of undeclared identifier 'no_such_var'
```

The real diagnosis is the line starting `error:`. Do not relay the first line to a user.

Keep cross-language expressions to field access, indexing and arithmetic, which work
everywhere. When you need a length, read it from the summary instead — codelldb renders
collections as `size=N` and delve as `[]int len: 3, cap: 3, [1,2,3]`.

## A portable stop handler

Pulling the rules together. Given the JSON from `continue --wait`:

```python
def handle_stop(reply):
    if reply["state"] != "paused":
        return reply["state"], reply.get("exit_code")

    # Rule 3: identify the stop by reason and position, not breakpoint ids.
    where = (reply["frame"]["source"].get("path"), reply["frame"]["line"])
    why = reply["reason"]          # "breakpoint" | "step" | "entry" | "exception" | ...

    # Rule 4: chunks are not lines.
    text = "".join(c["output"] for c in reply["captured_output"]
                   if c["category"] == "stdout").replace("\r\n", "\n")

    return why, where, text.splitlines()
```

and for reading a frame's variables:

```python
scopes = run("scopes")["scopes"]
locals_ref = scopes[0]["variables_reference"]        # Rule 1: positional
for v in run("variables", "--reference", str(locals_ref))["variables"]:
    name = v["name"]
    type_name = v.get("type_name")                   # Rule 2: may be absent
    value = v["value"]                               # a display string, not data
```

That last comment is the rule under the rules. `value` is written for a human glancing at a
debugger pane. Under codelldb it drops struct-typed fields with no ellipsis, floors a
`Duration` to the second, and truncates a Rust string at an embedded NUL. Anything you intend
to compute with should come from expanding `variables_reference`, on every adapter.

## Things that are the same

Worth stating, because the list above is longer than the list of real problems:

- `--stop-on-entry` reports `reason: "entry"` on all three. codelldb's underlying stop is a
  `SIGSTOP` that LLDB calls an exception, but lazydap normalises it and leaves the original in
  `raw_reason` — so `raw_reason: "exception"` under codelldb, `null` under the other two, and
  `reason` you can branch on everywhere.
- `state` is `paused` / `exited` / `terminated` / `timeout` / `adapter_died` regardless of
  adapter, and `exit_code` is the program's.
- Breakpoints persist in `.lazydap/state.toml` and are re-applied at every launch, in every
  language.
- The first scope is the locals scope.

## See also

- [Debug with an agent](/guides/agents/) — the loop these rules apply to
- [The `--wait` contract](/guides/wait/) — the five states, in full
- [Breakpoints](/guides/breakpoints/) — `verified`, and what makes one bind
- [JSON output](/reference/json-output/) — the field-by-field schema
- [codelldb quirks](/reference/codelldb-quirks/) — 21 entries, the adapter with the most of them
- [debugpy quirks](/reference/debugpy-quirks/) — 16 entries, mostly differences rather than faults
- [delve quirks](/reference/delve-quirks/) — 15 entries, mostly about launch arguments

Those three pages carry the forensic write-up behind every row in the table above: the captured
output, the cause, and what to do instead.
