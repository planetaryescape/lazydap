/* Crashes *inside libc*, not in its own code.
 *
 * `crashes.c` faults in `main`, so the frame the adapter reports is already
 * the one a person wants. This one passes a null key to `strcmp`, and the
 * segfault happens in `_platform_strcmp$VARIANT$Base` — a frame with no source
 * path at all, only a `source_reference`. Naming the code responsible for it
 * means walking down to `lookup_key`, which is what `user_frame` exists to do
 * (D078). */
#include <stdio.h>
#include <string.h>

struct entry {
    const char *key;
    int value;
};

static struct entry table[] = {
    {"alpha", 1},
    {"beta", 2},
    {NULL, 3}, /* the one that kills it */
};

int lookup_key(const char *wanted) {
    for (int i = 0; i < 3; i++) {
        if (strcmp(table[i].key, wanted) == 0) { /* line 25 — the crash */
            return table[i].value;
        }
    }
    return -1;
}

int main(void) {
    printf("looking up gamma\n");
    fflush(stdout);
    int found = lookup_key("gamma");
    printf("found %d\n", found);
    return 0;
}
