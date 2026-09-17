/* Textbook hygiene and performance: checks that can never fire.
 *
 * `n < 0` on an unsigned value is always false, so the error branch is dead
 * code and the intent (reject negatives) was lost at the signature. And in
 * `clamp`, the second check is implied by the first: the branch is redundant,
 * which is a cost on a hot path and a sign the invariants are muddled.
 */
#include <stdint.h>

// expect: reject_negative finding
// expect: clamp finding

int reject_negative(unsigned int n) {
    if (n < 0) return -1;
    return (int)(n & 0x7fffffffu);
}

uint32_t clamp(uint32_t v, uint32_t hi) {
    if (v > hi) v = hi;
    if (v > hi) return 0;      /* dead: v <= hi holds here */
    return v;
}
