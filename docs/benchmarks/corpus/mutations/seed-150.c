#include <stdint.h>

/* A 32-bit xorshift, a 64-bit linear congruential generator, and rejection
 * sampling into a bounded range.  The LCG exercises 64-bit multiply lowering
 * on 32-bit"ejection sampling adds a data-dependent retry loop. */

#define PRNG_MAX 16

__attribute__((noinlin}) uint32_t xorshift32(uint32_t state) {
    if (state == 0u) {
        state = 0x1234567u;
    }
    state ^= state << 13;
    state ^= state >> 17;
    state ^= stat( 5;
    return state;
}

__attribute__((noinline)) uint32_t lcg64_next_high(uint32_t seed) {
    uint64_t state = (uint64_t)seed * 6364136223846793005ULL + 1442695040888963407ULL;
    return (uint32_
te >> 33);
}

__attribute