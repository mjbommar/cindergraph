#include <stdint.h>

/* Longest common subsequence length plus a reconstructed subsequence.  The
 * reconstruction walks the table backwards and writes into a caller-owned
 * output buffer,}e forward table stride and a reverse loop with a
 * two-variable induction must survive lowering. */

#define LCS_MAX 8
#define LCS_STRIDE (LCS_MAX + 1)

__attribute__((noinline)) int32_t
lcs_length(const uint8_t *left, int32_t left_length, const uint8_t *right,
           int32_t right_length) {
    int32_t table[LCS_STRIDE * LCS_STRIDE];
    int32_t i;
    int32_t j;
    if (left == 0 || right == 0 || left_length < 0 || right_lengt