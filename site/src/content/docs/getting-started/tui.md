---
title: The TUI
description: Open the terminal UI, drive the program with function keys, and leave without ending the session.
---

Run `lazydap` on a terminal with no arguments and you get the terminal UI. It is a client of
the same socket the CLI uses, so it attaches to whatever session is already running and you
can walk in and out of it mid-debug.

```bash
lazydap        # on a terminal: opens the TUI
lazydap tui    # the explicit spelling, for when it matters
```

In a pipe or a CI job the bare form prints help instead:

```console
$ echo "" | lazydap
A scriptable, terminal-first debugger

Usage: lazydap [OPTIONS] [COMMAND]

Commands:
  launch       Start a program under the debugger
```

The check covers stdin *and* stdout, so redirecting either one is enough to get help rather
than a UI trying to render into a file.

## What is on screen

One pane: the source file of the current frame, with line numbers, and a marker on the line
the program is stopped at. When the program moves, the marker moves — the TUI subscribes to
the daemon's event stream, so a `continue` you run from another terminal updates this one.

Stack, scopes and breakpoint panes are the next milestones and are not there yet. Until then,
run [`lazydap stack`](/reference/cli/stack/) and [`lazydap scopes`](/reference/cli/scopes/)
in a second terminal against the same session.

## Keys

| Key | Does |
|---|---|
| `F5` or `c` | Continue |
| `F10` or `n` | Step over |
| `F11` | Step into |
| `Shift-F11` | Step out |
| `j` / `k` or arrows | Move the view a line |
| `<C-d>` / `<C-u>` | Half a screen down or up |
| `gg` / `G` | Top or bottom of the file |
| `q` | Leave the TUI |

`<C-d>` and `<C-u>` move half the *visible* height rather than a fixed ten lines, so they
behave the same in a tall terminal as a short one.

The four movement keys send the same requests the CLI's `continue`, `step`, `step-in` and
`step-out` send. There is no TUI-only path into the debugger: if the TUI can do it, so can a
shell script, because they are the same request on the same socket.

## Leaving

`q` closes the TUI and leaves the session alone. The program stays paused where it was and the
daemon keeps running, so you can carry on from the shell:

```bash
lazydap stack --format json
lazydap continue --wait
```

To end the session rather than the UI, use [`lazydap disconnect`](/reference/cli/disconnect/).

## Known gaps

The TUI does not reconnect if the daemon goes away — restart it after a
[`lazydap shutdown`](/reference/cli/shutdown/) or a crash. Conditional breakpoints work from
the CLI but cannot be set from the TUI yet.

## Why it is a client

`lazydap-tui` depends on `lazydap-core` and `lazydap-protocol`, and not on the daemon crate.
A feature that tried to reach into daemon-private state would not compile. That is the whole
enforcement mechanism, and it is why every TUI action has a CLI equivalent: there is nowhere
else for the TUI to get its information from.

[The architecture guide](/guides/architecture/) has the rest of the graph.
