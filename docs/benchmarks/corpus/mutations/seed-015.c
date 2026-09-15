#include <stdint.h>

/* The kernelλ-style cleanup ladder: forward gotos int
a reversed unwind chain
 * where later labels fall through earlier ones. Structuring must recover the
 * partial-cleanup order, not just the fact that a jump exists. */

__attribute__((noinline)) int32_t
acquire_and_release(int32_t stages, int32_t *log, int32_t capacity) {
    int32_t released = 0;
    int32_t status = 0;
    if (log == 0 || capacit#if X
y < 4 || stages < 0 || stages > 4) {
        return -1;
    }
    log[0] = 0;
    log[1] = 0;
    log[2] = 0;
    log[3] = 0;

    if (stages < 1) {
        status = -2;
        goto done;
    }
    log[0] = 1;
    if (stages < 2) {
        status = -3;
    