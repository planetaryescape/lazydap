"""Runs to completion and exits cleanly. The `state: exited` case."""

x = 5
print(f"about to finish x={x}", flush=True)  # line 4 — breakpoint line
raise SystemExit(0)
