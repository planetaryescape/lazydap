/* A stop with something worth inspecting.
 *
 * `big` is large enough that `variables --start/--count` has to actually
 * narrow something. `nowhere` points at an address no process can read, which
 * is how codelldb is made to answer an `evaluate` with an error hidden inside
 * a successful response — `*nowhere` gives the `<... failed ...>` shape lazydap
 * deliberately still reports as a value, and `*(int *)0` gives the `<error:`
 * shape it fails on (D068, D074). */
#include <stdio.h>

int main(void) {
    int big[2000];
    for (int i = 0; i < 2000; i++) {
        big[i] = i;
    }

    int *nowhere = (int *)4;
    int sum = big[0] + big[1999];

    printf("sum %d at %p\n", sum, (void *)nowhere); /* line 20 — breakpoint line */
    fflush(stdout);
    return 0;
}
