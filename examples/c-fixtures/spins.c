/* Never stops on its own. The `state: timeout` case.
 *
 * The sleep is not decoration: without it this burns a core for as long as a
 * test waits it out, and CI machines are shared. */
#include <stdio.h>
#include <unistd.h>

int main(void) {
    printf("spinning\n");
    fflush(stdout);
    for (;;) {
        usleep(20000);
    }
    return 0;
}
