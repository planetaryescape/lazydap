/* Dereferences a null pointer. The exception / terminated case. */
#include <stdio.h>

int main(void) {
    printf("about to crash\n");
    fflush(stdout);
    int *nowhere = 0;
    *nowhere = 1; /* line 8 — the segfault */
    return 0;
}
