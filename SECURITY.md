# Security

## Supported versions

| Version | Supported |
|---|---|
| 0.1.x | Yes, once released |
| < 0.1 | No |

lazydap is pre-release: `v0.1.0` has not been tagged and there are no published binaries. Until it is, "supported" means the tip of `main`. Fixes land there and there is no backport branch.

## Reporting a vulnerability

Use GitHub's private reporting: **[Security → Report a vulnerability](https://github.com/planetaryescape/lazydap/security/advisories/new)** on this repository. It opens a private advisory visible only to you and the maintainers. Don't open a public issue for anything you think is exploitable.

Include what you did, what happened, and what you expected. A reproduction against a known codelldb version on a named OS is worth more than a description, because most of the interesting surface is in the interaction between lazydap and the adapter.

Expect an acknowledgement within a week. If a report is valid you'll be credited in the advisory unless you'd rather not be.

## What lazydap does, so you can judge what matters

**A debugger runs arbitrary code on purpose.** `lazydap launch ./program` starts your program under codelldb, with your uid and your environment. `lazydap eval` evaluates an expression inside that process, which can call functions in it. Setting a breakpoint patches the running process. All of this is the feature. A report saying lazydap can be made to execute code is describing the product.

**The trust boundary is the socket.** Every client talks to the daemon over a Unix domain socket, and a `Launch` request naming a program and arguments arrives that way. There is no authentication on that channel and no peer-credential check: anyone who can open the socket can start any program as you. What stands between them and that is filesystem permissions.

- The socket lives at `$LAZYDAP_RUNTIME_DIR` or `$XDG_RUNTIME_DIR/lazydap` (Linux) or `/tmp/lazydap-<uid>` (macOS), one per project instance.
- That directory is created `0700` at creation time, re-chmodded to `0700` if it's found looser, and rejected outright if it's a symlink, not a directory, or not owned by your uid. The check uses `lstat` and does not follow links.
- The socket file is set to `0600` immediately after bind.
- The daemon's log is opened `0600` and lives under `$LAZYDAP_DATA_DIR` or your platform data directory. It records the paths of programs you debugged and the output they produced.

**No network listener.** lazydap opens exactly one outbound socket, to `127.0.0.1` on the port the codelldb child prints on startup, and lazydap spawns that child itself. It binds nothing on a network interface, makes no HTTP requests, and has no HTTP client in its dependency tree. See [`PRIVACY.md`](PRIVACY.md).

**The adapter binary is not configurable.** The daemon looks up the literal name `codelldb` on `PATH` and runs the first executable regular file it finds. No config file or state file can name a different executable in this build. Your `PATH` is trusted, as it is by everything else you run.

**Project state is not privileged.** `.lazydap/state.toml` holds breakpoints (source path and line). It is created with your default umask, not `0600`, on the theory that a team may want to commit it. It cannot name a program, arguments, or an environment, so a hostile copy of it cannot cause anything to be executed.

## What is a vulnerability here

Report it if you find:

- A way to reach the daemon without permission to open its socket, or a path where the socket or runtime directory ends up world- or group-accessible.
- A malformed frame on the socket that crashes the daemon, hangs it, or reads memory it shouldn't.
- Anything in a state file, a project path, a source path, or an adapter message that escapes its context: a path traversal out of the project root, an injection into a spawned command line, an unbounded allocation from an attacker-controlled length.
- The daemon acting on a request from a session or instance it doesn't belong to.
- Secrets ending up in the log or in JSON output that a caller wouldn't expect there.
- lazydap running something the caller didn't name.

## What is not

- **Debugging arbitrary programs, or evaluating expressions inside them.** That is the tool.
- **A program you debugged doing something bad.** lazydap ran it because you asked.
- **codelldb or LLDB bugs.** Report those to [vadimcn/codelldb](https://github.com/vadimcn/codelldb/issues) or LLVM. If lazydap's handling turns an adapter bug into something worse, that part is ours.
- **Anything requiring an attacker who already has your uid.** They can read the socket, the log, and your source. They can also run your program directly. There is no boundary left to cross.
- **A `PATH` you control being used to find `codelldb`.** An attacker who can write to your `PATH` owns your shell already.

## Known gaps, stated rather than hidden

- **The socket is chmodded after bind, not before.** There is a brief window at umask-default mode between `bind` and `set_permissions`. On a `0700` parent directory, nothing else can reach it during that window.
- **There is a TOCTOU gap** between validating the runtime directory and binding inside it. Closing it properly needs `openat`, which would mean a `libc` dependency. It is recorded in the source rather than fixed.
- **No peer-credential check on connections.** Access control is filesystem permissions only, which is the same posture as the directory the socket sits in.
- **`.lazydap/` and `state.toml` get default umask permissions**, unlike the runtime and data directories.
