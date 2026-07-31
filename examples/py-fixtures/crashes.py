"""Dies on an uncaught exception. Python's answer to c-fixtures/crashes.c.

There is no segfault to reach from Python, and an uncaught exception is the
same thing at this level: the program stops because of something it did, not
because it was asked to, and the process exits non-zero.
"""

print("about to fail", flush=True)
raise ValueError("nothing here")  # line 9 — the failure
