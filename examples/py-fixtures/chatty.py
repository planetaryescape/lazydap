"""Prints a great deal, then stops. For captured_output volume and ordering."""

for i in range(200):
    print(f"line {i}")

done = 1
print("finished", flush=True)  # line 7 — breakpoint line
raise SystemExit(done - 1)
