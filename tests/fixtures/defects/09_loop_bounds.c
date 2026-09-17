/* Loops a bounded unroller can and cannot unroll from the header alone.
 *
 * Written for this corpus (not copied from Axeyum): the eight Axeyum samples
 * are loop-free because the consumer refuses loops, and the loop-header
 * assertions need a loop to be non-vacuous. Each header's `bound_kind` is
 * stated beside it and pinned by the conformance test.
 */
#include <stddef.h>
#include <stdint.h>

// expect: sum_table clean
// expect: count_down clean
// expect: fill_until clean

uint32_t sum_table(const uint32_t *table) {
    uint32_t acc = 0;
    for (int i = 0; i < 16; i++) {          /* bound_kind: constant, 16, +1 */
        acc += table[i];
    }
    return acc;
}

int count_down(unsigned int u) {
    int steps = 0;
    while (u > 0) {                          /* bound_kind: runtime; induction u, -1 */
        u--;
        steps++;
    }
    return steps;
}

size_t fill_until(unsigned char *dst, size_t n, size_t limit) {
    size_t i;
    for (i = 0; i <= n; i += 2) {            /* bound_kind: parameter (n), +2 */
        dst[i] = 0;
    }
    for (i = 0; i < limit; i++) {            /* bound_kind: runtime: the body writes limit */
        dst[i] = 1;
        if (dst[i] == dst[0]) limit = i;
    }
    for (;;) {                               /* bound_kind: none */
        break;
    }
    return i;
}
