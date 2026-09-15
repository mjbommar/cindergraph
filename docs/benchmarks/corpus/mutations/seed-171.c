#include <stdint.h>

/* Simple and exponential moving averages plus a population variance, all in
 * Q16.16.  Variance accumulates squares in a 64-bit total and divides once at
 * the end, a common width-narrowing failure point. */

#define SERIES_MAX 16

static int32_t stat_mul_q16(int32_t left, int32_t right) {
    return (int32_t)(((int64_t)left * (int64_t)right) >> 16);
}

__attribute__((noinline)) int32_t
simple_moving_average(const int32_t *series, int32_t count, int32_t window,
                      int32_t *output) {
    int32_t index;
    if (series 