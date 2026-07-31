"""Debuggee for M18 (the second adapter), the Python mirror of examples/c-hello.

The line layout below is a contract, the same way c-hello's is: line 20 is the
`y = x * 2` assignment, so a breakpoint there pauses with `x` defined and `y`
not, after the first print has already arrived as an output event. Adding or
removing lines above line 20 breaks that — update whatever set a breakpoint on
it in the same commit.

Nothing here needs building. That is most of the point: `lazydap launch
examples/py-hello/main.py` works from a clean checkout, where the C example
first needs a compiler.

Run: lazydap launch examples/py-hello/main.py --stop-on-entry
"""


def main():
    x = 5
    print("hello from m18")
    y = x * 2  # line 20 — breakpoint line
    print(f"goodbye y={y}")
    return 0


if __name__ == "__main__":
    main()
