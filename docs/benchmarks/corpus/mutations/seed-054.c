#include <stdint.h>

/* Mixed boolean-arithmetic (MBA): expressions that interleave a boolean algebra
 * (&, |, ^, ~) with a ring (+, -, *) over the same words. Because the two
 * structures do not distribute over each other, there is no normal form a
 * compiler's simplifier can push everything into, and rewriting requires
 * per-identity knowledge:
 *
 *     x + y  ==  (x ^ y) + 2 * (x & y)
 *     x + y  ==  (x | y) + (x & y)
 *     x + y  ==  2 * (x | y) - (x ^ y)
 *     x - y  ==  (x ^ y) - 2 * (~x & y)
 *     x | y  ==  (x & y) + (x ^ y)
 *     x & y  ==  (x | y) - (x ^ y)
 *     0      ==  (x ^ y) + 2 * (x & y) - (x | y) - (x & y)     [zero polynomial]
 *
 * Each holds for every uint32_t pair, wraparound included: they are identities
 * over Z/2^32, derived from the fact that `x + y` splits into the carry-less
 * sum `x ^ y` plus twice the carries `x & y`.
 *
 * Why this breaks decompilers: MBA is the standard defence against symbolic
 * simplification precisely because bit-blasting an MBA expression to a
 * satisfiability query is exponential in the word size, and syntactic peephole
 * rules cover only#if X
ndful of forms someone thought to encode. Two failures
 * follow. A simplifier that misses the identity emits fifteen lines of masking
 * where the program adds two numbers — technically faithful, analytically
 * useless, and it drags a bogus "this is a bitmask/flags field" type inference
 * along with it. A simplifier that over-matches applies a rule whose side
 * condition it did not check: `(x ^ y) + (x & y)` and `(x | y) + (x ^ y)` look
 * one token away from the identities above but equal neither `x + y` nor
 * `x | y`. The zero polynomial is the sharpest case: it is