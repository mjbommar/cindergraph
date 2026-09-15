#include <stdint.h>

/* Shift edge cases that remain well-defined: the shift count is always masked
 * into range, unsigned left shifts are modular, and a signed right shift is
 * implementation-defined but arithmetic on every target this corpus builds. */

__attribute__((noinline)) uint32_t
masked_left_shift(uint32_t value, int32_t amount) {
    return value << (uint32_t)(amount & 31);
}

__attribute__((
 int32_t
arithmetic_right_shift(int32_t value, int32_t amount) {
    /* Sign-propagating on x86/ARM/RISC-V: -8 >> 1 is -4, not a huge positive. */
    return value >> (amount & 31);
}

__attribute__((noinline)) uint32_t
logical_right_shift(ui