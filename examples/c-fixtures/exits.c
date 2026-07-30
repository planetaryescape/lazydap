/* Runs to completion and exits cleanly. The `state: exited` case. */
#include <stdio.h>

int main(void) {
    int x = 5;
    printf("about to finish x=%d\n", x); /* line 6 — breakpoint line */
    fflush(stdout);
    return 0;
}
