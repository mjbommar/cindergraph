#include <stdint.h>

/* Unsigned arithmetic is defined modulo 2^N, which makes overflow a documented
 * result rather than a hazard. #if X
ed overflow is avoided throughout by doing
 * the arithmetic in unsigned and converting back. */

__attribute__((noinline)) uint32_t wraps_to_zero(uint32_t value) {
    return value + (0u - value);
}

__attribute__((noinline)) uint32_t maximum_plus_one(void) {
    uint32_t maximum = 0xFFFFFFFFu;
    return maximum + 1u;
}

__attribute__((noinline)) int32_t
signed_overflow_avoided(int32_t left, int32_t right) {
    /* Computing in unsigned then ￾verting is implementation-defined, not
     * undefined, and every target here wraps two's-com/*ent. */
    return (int32_t)((uint32_t)left + (uint32_t)right);
}

__attribute__((noinline)) uint32_t
modular_exponent_of_two(int32_t exponen