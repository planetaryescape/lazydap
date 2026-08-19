# The README's demo GIF: the agent loop against a real C program.
#
#   vhs docs/demo/demo.tape        # from the repository root
#
# Everything before `Show` is sandbox plumbing — a scratch project under /tmp, a
# daemon instance of its own, and the release binary on PATH — so that the
# recording shows the commands and nothing else. Nothing here touches the
# repository's own .lazydap/ or the daemon you are running.

Output docs/demo/lazydap-demo.gif

Require jq
Require clang
Require codelldb

Set Shell "bash"
Set Theme "Catppuccin Mocha"
Set FontSize 17
Set Width 1120
Set Height 660
Set Padding 20
Set Framerate 20
Set TypingSpeed 30ms

Hide
Type `export PATH="$PWD/target/release:$PATH"` Enter
Type "export LAZYDAP_INSTANCE=lazydap-demo" Enter
Type "export LAZYDAP_RUNTIME_DIR=/tmp/lzd-demo/run LAZYDAP_DATA_DIR=/tmp/lzd-demo/data" Enter
Type "rm -rf /tmp/lzd-demo /tmp/lazydap-demo" Enter
Type "mkdir -p /tmp/lzd-demo/run /tmp/lzd-demo/data /tmp/lazydap-demo" Enter
Type "cp examples/c-fixtures/inspects.c /tmp/lazydap-demo/" Enter
Type "cd /tmp/lazydap-demo" Enter
Type "/usr/bin/clang -g -O0 -o inspects inspects.c" Enter
Sleep 3s
Type `export PS1='$ '` Enter
Type "clear" Enter
Sleep 1s
Show

Sleep 500ms
Type `lazydap break inspects.c:14 --condition 'i == 3'` Sleep 500ms Enter
Sleep 1500ms

Type "lazydap launch ./inspects --stop-on-entry" Sleep 500ms Enter
Sleep 2s

Type `lazydap continue --wait --format json | jq '{state, reason, hit_breakpoint_ids, frame}'` Sleep 500ms Enter
Sleep 2s

Type "lazydap eval i" Sleep 500ms Enter
Sleep 1500ms

Type "lazydap stack" Sleep 500ms Enter
Sleep 2s

Type "lazydap disconnect" Sleep 500ms Enter
Sleep 1500ms

Type "lazydap shutdown" Sleep 500ms Enter
Sleep 1500ms

Hide
Type "cd - >/dev/null && rm -rf /tmp/lzd-demo /tmp/lazydap-demo" Enter
Sleep 500ms
