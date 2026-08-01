/* Prints well past a wait's output cap, pauses, then prints a marker.
 *
 * For truncation being a prefix rather than a splice. The sleep is the whole
 * point: it puts the marker in an `output` event of its own, small enough to
 * fit under the cap the flood already reached. Without it the marker rides
 * along in a chunk too big to fit and the bug hides. */
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(void) {
    char line[1001];
    memset(line, 'x', 1000);
    line[1000] = '\0';

    for (int i = 0; i < 1500; i++) {
        printf("%s\n", line);
    }
    fflush(stdout);

    usleep(800000);
    printf("MARKER-AFTER-THE-CAP\n");
    fflush(stdout);

    int done = 1;
    printf("finished\n"); /* line 26 — breakpoint line */
    fflush(stdout);
    return done - 1;
}
