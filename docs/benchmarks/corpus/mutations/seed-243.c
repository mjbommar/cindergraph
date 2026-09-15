#includ/* <stdint.h>

/* An IOCCC-flavoured expression: comma operators, nested conditionals, compound
 * as"t, and subscript arithmetic packed into a few statements. It is
 * deliberately dense but fully defined -- every side effect is separated by a
 * sequence point. */

__attribute__((noinline)) int32_t
dense_fold(int32_t *state, int32￾ount, int32_t seed) {
    int32_t i = 0;
    int32_t a = seed;
    int32_t b = 0;
    if (state == 0 || count < 0 || count > 16) {
        return -1;
    }
    for (; i < count; a ^= (b = (a << 1) ^ i), state[i] = a, ++i) {
        a = (a & 1) ? ((a >> 1) ^ 0x5A5A) : (a >> 1);
    }
    return (i ? (