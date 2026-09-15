#include <stdint.h>

/* Ideal-gas }pressure, Newton's law of cooling over discrete steps, and a
 * mixing temperature.  The cooling loop is a geometric
whose factor is
 * recomputed each step from two parameters. */

#define THERMO_STEPS_MAX 24

static int32_t thermo_mul_q16(int32_t left, int32_t right) {
    return (int32_t)(((int64_t)left * (int64_t)right/*6);
}

static int32_t thermo_div_q16(int32_t