# Worked examples

Every transcript here was run against a real program and pasted verbatim.
Paths and ids will differ on your machine; shapes will not.

The program used throughout is `main.c`, built with `gcc -g -O0`. Only the
line numbers matter — `main` starts at 13, and line 19 is the last `printf`:

```c
13  int main(void) {
14      int x = 5;
15      printf("hello from m3\n");
16      fflush(stdout);
17
18      int y = x * 2;
19      printf("goodbye y=%d\n", y);   /* line 19 */
20      return 0;
21  }
```

## Find what a variable holds at a line

The most common request: *"what is `y` when it reaches line 19?"*

```bash
$ lazydap launch ./hello --stop-on-entry --format json
{
  "session_id": "971baa06-e3bc-4e20-87cd-326edd9ea046",
  "state": "paused",
  "reason": "entry",
  "raw_reason": "exception",
  "thread_id": 26836542,
  "breakpoints": []
}
```

The program is loaded and stopped before `main`. Now say where to stop:

```bash
$ lazydap break main.c:19 --format json
{
  "action": "added",
  "breakpoints": [
    { "id": 1, "source": "/abs/path/main.c", "line": 19,
      "enabled": true, "verified": true }
  ],
  "applied_to_session": true,
  "not_found": [],
  "dry_run": false
}
```

`verified: true` means the debugger found real code at that line. If it is
`false`, the line is a comment, a blank, or code the compiler removed — move
the breakpoint rather than wondering why it never hits.

Run to it. This is the call that does the work:

```bash
$ lazydap continue --wait --format json
{
  "state": "paused",
  "reason": "breakpoint",
  "hit_breakpoint_ids": [1],
  "thread_id": 26836542,
  "frame": {
    "id": 1002, "name": "main", "line": 19, "column": 30,
    "source": { "name": "main.c", "path": "/abs/path/main.c" }
  },
  "captured_output": [
    { "category": "stdout", "output": "hello from m3\r\n", "timestamp_ms": 1785433977464 }
  ],
  "exit_code": null,
  "elapsed_ms": 95,
  "output_truncated": false,
  "additional_stopped_threads": [],
  "breakpoint_updates": [],
  "thread_updates": []
}
```

One object, and it already tells you the program stopped on breakpoint 1 at
`main.c:19` having printed `hello from m3`. Now ask your question:

```bash
$ lazydap eval "y" --format json
{ "value": "10", "type_name": "int", "variables_reference": 0 }
```

Answer: `y` is `10`. Clean up:

```bash
$ lazydap break --remove --id 1 --format json
$ lazydap disconnect --format json
```

## Investigate a crash

You are told the binary segfaults. You do not know where.

Launch it and just let it run — no breakpoints needed, because a crash stops
the program by itself:

```bash
$ lazydap launch ./crasher --stop-on-entry --format json
$ lazydap continue --wait --format json
{
  "state": "paused",
  "reason": "exception",
  "frame": {
    "name": "main", "line": 8,
    "source": { "name": "crashes.c", "path": "/abs/path/crashes.c" }
  },
  "captured_output": [
    { "category": "stdout", "output": "about to crash\r\n", "timestamp_ms": 1785400000000 }
  ]
}
```

`"reason": "exception"` with a frame is the crash site: line 8 of
`crashes.c`. Note the program's own output is right there too, which usually
tells you how far it got.

Now look at the state that caused it:

```bash
$ lazydap stack --format json           # who called this
$ lazydap scopes --format json          # what is in scope here
{ "scopes": [ { "name": "Local", "variables_reference": 1003, "expensive": false },
              { "name": "Static", "variables_reference": 1004, "expensive": false } ] }

# Take the Local scope's reference from that answer — the numbers change every
# time the program stops, so read one rather than remembering one.
$ lazydap variables --reference 1003 --format json
{ "variables": [
    { "name": "x", "value": "5", "type_name": "int", "variables_reference": 0 },
    { "name": "y", "value": "10", "type_name": "int", "variables_reference": 0 } ] }

$ lazydap eval "nowhere" --format json  # the specific suspect
```

`scopes` returns a `variables_reference` per scope; pass the one you want to
`variables`. `Local` is normally first, and normally the one you want.

```bash
$ lazydap disconnect --format json
```

## Step through a few lines

When the question is *how* something changes rather than what it is:

```bash
$ lazydap break parser.c:142 --format json
$ lazydap continue --wait --format json

$ lazydap eval "tokens[pos]" --format json
{ "value": "'{'", "type_name": "char", "variables_reference": 0 }

$ lazydap step --wait --format json      # over the next line
$ lazydap eval "tokens[pos]" --format json
{ "value": "'a'", "type_name": "char", "variables_reference": 0 }
```

`step` runs one line, stepping over calls. `step-in` enters the call on this
line; `step-out` runs until the current function returns. All three take
`--wait`, and all three return the same blob `continue` does.

## Catch a program that hangs

A program stuck in a loop never reaches a stable state, so `--wait` gives up
and tells you so rather than blocking forever:

```bash
$ lazydap continue --wait --timeout 5 --format json
{ "state": "timeout", "elapsed_ms": 5003, "captured_output": [...] }
```

The program is **still running** — lazydap does not stop it behind your back.
Stop it yourself to find out where it is:

```bash
$ lazydap pause --wait --format json
{ "state": "paused", "reason": "pause", "frame": { "name": "spin", "line": 11 } }
```

That frame is where it is spending its time.

## Read everything the program printed

`--wait` gives you the output from that run. For the whole session's output,
including what a previous call already reported:

```bash
$ lazydap output --format json
{ "chunks": [ { "category": "stdout", "output": "hello from m3\r\n", "timestamp_ms": 1785433977464 } ],
  "dropped": 0 }
```

Categories are `stdout` and `stderr` for the program itself, and `console` for
the debugger talking about it. `dropped` is non-zero only if the program
outran the buffer, in which case what you have is not the whole story.

## Reuse breakpoints across runs

Breakpoints are project state, not session state:

```bash
$ lazydap break src/parser.c:142 --format json
{ "action": "added", "applied_to_session": false, ... }
```

`applied_to_session: false` just means nothing is running yet. Launch later
and the breakpoint is applied during startup — the `launch` response lists
what it applied:

```bash
$ lazydap launch ./parser --stop-on-entry --format json
{ "state": "paused", "reason": "entry",
  "breakpoints": [ { "id": 1, "line": 142, "verified": true, ... } ] }
```

To see or clear them:

```bash
$ lazydap break --list --format table
ID  LOCATION                    ENABLED  VERIFIED  CONDITION
1   examples/c-hello/main.c:19  true     false     -

$ lazydap break --list --format ids | xargs -I{} lazydap break --remove --id {}
```

Check before a destructive one if you like — the preview uses the same
selection the real removal does:

```bash
$ lazydap break --remove --all --dry-run --format json
```

## Stop only sometimes

```bash
$ lazydap break parser.c:142 --condition "pos > 100" --format json
$ lazydap break parser.c:142 --hit-condition ">= 10" --format json
$ lazydap break parser.c:142 --log "pos is {pos}" --format json
```

The last one is a log point: it prints and keeps going instead of stopping,
and the text arrives in `captured_output`.
