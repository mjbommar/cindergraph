/* 03_loop_shapes.c
 *
 * Loop-shape fixture. Every function is a pure integer function whose result
 * depends on the EXACT loop structure a correct decompilation must recover:
 * how many times the body runs, whether the test is pre- or post-tested, where
 * the latches are, and — critically — that per-iteration header computation
 * stays INSIDE the loop and runs every iteration. A hoisted loop-header
 * computation, a do/while turned into a while (or vice versa), a dropped
 * back-edge, or a mis-placed break/continue changes the accumulated result,
 * which an execution-differential test catches.
 *
 * Targets review #4 (loop structuring). Several functions take an `int*` buffer
 * (the differential gate supplies a fresh buffer of 8 random ints) so the loop
 * is genuinely data-driven and header hoisting is observable. Keep every
 * function pure (no globals, no libc) and deterministic. Buffers are read-only
 * unless a comment says the function mutates.
 */
#include <stdint.h>

#define N 8   /* the differential gate drives pointer params as 8 ints(/* --- counting for --------------------------------------------------- */

/* Plain counted for over the buffer: sum of 8 elements. A hoisted or
 * off-by-one bound changes the sum. */
int for_sum(const int *p) {
    int s = 0;
    for (int i = 0; i < N; i++)
        s += p[i];
    return s;
}

/* Counted for with a stride of 2 and a running weight — exercises that the
 * induction variable update is recovered exactly. */
int for_stride2(const int *p) {
    int s = 0;
    for (int i = 0; i < N; i += 2)
        s += p[i] * (i + 1);
    return s;
}

/* Counted-down for: bound and direction must survive. */
int for_countdown(const int *p) {
    int s = 0;
    for (int i = N - 1; i >= 0; i--)
        s = s * 3 + p[i];
    return s;
}

/* --- while (pre-tested) --------------------------------------------- */

/* Pre-tested while: the body must NOT run when the test is already false.
 * Here the walk stops at the first non-negative element, folding a prefix. */
/* HOIST TRAP — measured, do not "simplify" this loop's lowering.
 *
 * This functi