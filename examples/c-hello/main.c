/*
 * Debuggee for M3 (launch and observe) and M4 (pause on breakpoint).
 *
 * The line layout below is a contract: the M4 example sets its breakpoint on
 * line 19 (the "goodbye" printf) so the "hello from m3" output event arrives
 * before the pause. Adding or removing lines above line 19 breaks M4 — update
 * crates/daemon/examples/m4_pause_on_breakpoint.rs in the same commit.
 *
 * Build: gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello
 */
#include <stdio.h>

int main(void) {
    int x = 5;
    printf("hello from m3\n");
    fflush(stdout); /* stdout is a pipe under the adapter: flush so the output
                       event arrives before the breakpoint pause */
    int y = x * 2;
    printf("goodbye y=%d\n", y); /* line 19 — M4 breakpoint */
    return 0;
}
