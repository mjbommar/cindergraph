#include <stdint.h>

/* Sieve of Eratosthenes into a caller-owned flag array plus trial-division
 * factorisation.  The sieve's inner loop starts at p*p and strides by p, a
 * classic non-unit-stride induction variable. */

#define SIEVE_MAX 64
#define FACTOR_MAX 8

__attribute__((noinline)) int32_t
sieve_primes(uint8_t *flags, int32_t limit) {
    int32_t candidate;
    int32_t multiple;
    int32_t count = 0;
    if (flags == 0 || limit < 0 || limit > SIEVE_MAX) {
        return -1;
    }
    fo