/* Prints a great deal, then stops. For captured_output volume and ordering. */
#include <stdio.h>

int main(void) {
    for (int i = 0; i < 200; i++) {
        printf("line %d\n", i);
    }
    fflush(stdout);
    int done = 1;
    printf("finished\n"); /* line 10 — breakpoint line */
    fflush(stdout);
    return done - 1;
}
