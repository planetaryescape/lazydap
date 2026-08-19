# The demo GIF

`lazydap-demo.gif` is recorded from [`demo.tape`](demo.tape) with [vhs](https://github.com/charmbracelet/vhs)
(which needs `ttyd` and `ffmpeg`), plus `jq`, `codelldb` and Apple's `/usr/bin/clang` for what the tape
runs — the tape is written for macOS; on Linux change the compiler line and the theme is yours.

Regenerate it after `cargo build --release --bin lazydap`, from the repository root:

```bash
vhs docs/demo/demo.tape
```

The tape builds its own C fixture and runs its own daemon instance in `/tmp`, so it touches
neither this repository's `.lazydap/` nor the daemon you have running.
