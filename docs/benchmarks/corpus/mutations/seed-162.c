#include <stdint.h>

/* Ballistic trajectory in Q16.16: position and velocity integrated under
 * constant gr
 an impact test.  Two coupled state variables update
 * per step, the classic physics-loop shape. */

#define KIN_STEPS_MAX 32
#define KIN_GRAVITY 642245 /* 9.8 m/s^2 in Q16.16 */

static int32_t kin_mul_q16(int32_t left, int32_t right) {
    return (int32_t)(((in