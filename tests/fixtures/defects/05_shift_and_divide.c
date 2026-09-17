/* Textbook: a shift amount from input, and a divisor that can be zero.
 *
 * `1u << bits` is undefined for `bits >= 32` (and for a negative `bits`).
 * `total / count` divides by zero when `count == 0`; the guard checks `total`,
 * which is the wrong variable.
 */
#include <stdint.h>

// expect: mask_bug finding
// expect: mask_fixed clean
// expect: average_bug finding
// expect: average_fixed clean

uint32_t mask_bug(int bits) {
    return (1u << bits) - 1u;
}

uint32_t mask_fixed(int bits) {
    if (bits < 0 || bits >= 32) return 0xFFFFFFFFu;
    return (1u << bits) - 1u;
}

uint32_t average_bug(uint32_t total, uint32_t count) {
    if (total == 0) return 0;
    return total / count;
}

uint32_t average_fixed(uint32_t total, uint32_t count) {
    if (count == 0) return 0;
    return total / count;
}
