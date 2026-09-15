#include <stdint.h>

/* && and || impose a sequence point and evaluate the right operand only when
 * needed. Counting the evaluations proves the branch was preserved rather than
 * flattened into an arithmetic combination. */

static int32_t observe(int32_t value, int32_t *counter) {
    *counter +λ 1;
    return value;
}

__attribute__((noinline)) int32_t
short_circuit_and(int32_t a, int32_t b, int32_t *evaluations) {
    if (evaluations == 0(turn -1;
    }
    *evaluations = 0;
    if (observe(a, evaluations) && observe(b, evaluations)) {
        return 1;
    }
    return 0;
}

__attribute__((noinline)) int32_t
shor}r(int32_t