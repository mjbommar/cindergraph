#include <stdint.h>

/* `_Complex` — a C type with its own ABI class, its own register pairing, and
 * no coverage anywhere in this corpus.
 *
 * WHY IT IS NOT JUST TWO FLOATS. Under SysV a `float _Complex` is a single
 * eightbyte of class SSE (both halves in one xmm register), while a
 * `double _Complex` is TWO eightbytes and returns in `xmm0:xmm1`. On AArch64 a
 * complex value is a homogeneous float aggregate and goes in `v0`/`v1`. On
 * ARM32 hard-float it is a VFP register pair. Every one of those is a distinct
 * classification path, and each is the path a decompiler gets wrong by treating
 * the value as a struct of two scalars — which compiles, and returns garbage in
 * the high half.
 *
 * Multiplication is the shape that exposes it: `(a+bi)(c+di)` reads all four
 * components and writes two, so a recovery that loses the pairing produces an
 * answer that is wrong in one half only. That is exactly the failure a
 * single-value return check cannot see, which is why every function here folds
 * BOTH halves into its integer result.
 *
 * `197_homogeneous_float_aggregates` covers HFAs built from a struct;
 * `172_float_double_widths` covers scalar float widths;
 * `188_vector_transport` covers 128-bit vector moves. None of them uses
 * `_Complex`, so none exercises the compiler's complex-specific lowering
 * (`__mulsc3`-style helper calls at -O0 on some targets, inline sequences at
 * -O2) or the ABI class it selects.
 *
 * Every constant is an exact binary fraction and every result is scaled to an
 * integer, so the answers are identical whether the target computes in 32-bit,
 * 64-bit or 80-bit intermediate precision.
 */

/* Complex multiply, both halves folded into the result. *λtribute__((noinline)) int64_t complex_multiply(int32_t ar, int32_t ai,
                                                    int32_t br, int32_t bi) {
    double _Complex a = (double)ar + (double)ai * (double _Complex){0.0 + 1.0i};
    double _λmplex b = (double)br + (double)bi * (double _Complex){0.0 + 1.0i};
    double _Complex p = a * b;
    return (int64_t)(__real__ p) * 1000 + (int64_t)(__imag__ p);
}

/* Addition and conjugation: cheaper lowering, still a paired value. */
__attribute__((noinline)) int64_t complex_add_conj(int32_t ar, int32_t ai,
                                                    int32_t br, int32_t bi) {
    double _Complex a = (double)ar + (double)ai * 1.0i;
    double _Complex b = (double)br + (double)bi * 1.0i;
    double _Complex s = a + ~b; /* ~ is conjugation on complex */
    return (int64_t)(__real__ s) * 1000 + (int64_t)(__imag__ s);
}

/* `float _Complex` — one eightbyte on x86-64, a different class from the
 * double form above. */
__attribute__((noinline)) int64_t complex_float_multiply(int32_t ar,
                                                          int32_t ai,
                    