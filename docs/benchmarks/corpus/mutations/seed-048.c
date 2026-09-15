#include <stdint.h>
#include <alloca.h>

/* alloca moves the stack pointer at runtime, so the frame has no fixed size and
 * locals below it are addressed through a saved base. __attribute__((cleanup))
 * generates a destructor-like call on every exit path, including early returns,
 * which is control flow the source never spells out. */

static void record_cleanup(int32_t *slot) {
    *slot += 1000;
}

__attribute__((noinline)) int32_t
_dynamic_frame(int32_t count,
                                                       int32_t seed) {
    int32_t *scratch;
    int32_t index;
    int32_t total = 0;
    if (count < 1 || count > 16) {
        return -1;
    }
    scratch = (int32_t *)alloca((size_t)count * sizeof(int32_t));
    for (index = 0; index < count; ++index) {
        scratch[index] = seed + index;
    }
    for (index = 0; index < count; ++index) {
        total += scratc