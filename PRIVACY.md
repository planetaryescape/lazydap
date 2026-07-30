# Privacy

lazydap runs on your machine and talks to nothing.

**No telemetry.** No analytics, no usage counters, no crash reporting, no update checks, no version pings. There is no code that sends anything anywhere, and nothing to opt out of.

**No network.** lazydap opens one socket, to `127.0.0.1`, on a port that the codelldb process lazydap itself just started printed on startup. It binds nothing on a network interface. Its dependency tree contains no HTTP client and no TLS stack — the whole list is tokio, clap, ratatui, serde, toml, tracing, uuid and their support crates.

**No accounts.** Nothing to sign in to.

## What is written to disk, and where

Everything lazydap writes is local, and everything in it came from your own machine.

| What | Where | Contains |
|---|---|---|
| Daemon log | `$LAZYDAP_DATA_DIR`, or your platform data directory (`~/Library/Application Support/lazydap` on macOS, `$XDG_DATA_HOME/lazydap` on Linux) | Paths of programs you debugged and the output they produced. Opened `0600`, inside a `0700` directory. |
| Socket, lock, pid file | `$LAZYDAP_RUNTIME_DIR`, or `$XDG_RUNTIME_DIR/lazydap` on Linux, `/tmp/lazydap-<uid>` on macOS | Nothing about your code. The socket is `0600`, its directory `0700`. |
| Project state | `.lazydap/state.toml` under your project root | Your breakpoints: source path and line number. Nothing else. |

The log is the one to know about, because a debug session's output is your program's output. `lazydap logs` reads it. Deleting it is safe; the daemon makes a new one.

Live session state — frames, threads, variables, everything you inspect — is held in the daemon's memory and dies with the session. It is never written down.

## Removing it all

```bash
lazydap shutdown
rm -rf ~/Library/Application\ Support/lazydap    # macOS; $XDG_DATA_HOME/lazydap on Linux
rm -rf .lazydap                                  # per project, if you want the breakpoints gone
```

Questions about any of this belong in a GitHub issue. Suspected leaks belong in [`SECURITY.md`](SECURITY.md)'s private reporting flow.
