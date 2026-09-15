#include <stdint.h>

/* Pascal's triangle, binomial coefficients by multiplicative recurrence, and
 * Catalan numbers.  The multiplicative binomial divides exactly at each step,
 * so an incorrectly ordered multiply/divide overflows and diverges. */

#define PASCAL_MAX 12

__attribute__((noinline)) int32_t
p