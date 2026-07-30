/* Four threads all reaching the same line, for the coalescing window.
 *
 * They sleep the same amount before it on purpose: the point is to have
 * several arrive close enough together that the adapter reports them as
 * separate stops. */
#include <pthread.h>
#include <stdio.h>
#include <unistd.h>

#define WORKERS 4

static void *work(void *arg) {
    int id = *(int *)arg;
    usleep(50000);
    printf("worker %d\n", id); /* line 15 — breakpoint line */
    fflush(stdout);
    return NULL;
}

int main(void) {
    pthread_t threads[WORKERS];
    int ids[WORKERS];

    for (int i = 0; i < WORKERS; i++) {
        ids[i] = i;
        pthread_create(&threads[i], NULL, work, &ids[i]);
    }
    for (int i = 0; i < WORKERS; i++) {
        pthread_join(threads[i], NULL);
    }
    printf("all joined\n");
    fflush(stdout);
    return 0;
}
