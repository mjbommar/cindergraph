#include <stdint.h>

/* Scale stress: switch width.
 *
 * `wide154_dense_switch` has 256 contiguous cases returning distinct
 * constants, which compilers lower to a constant lookup table rather than
 * a jump table -- the switch disappears into an array the decompiler has to
 * find. `wide154_dense_effects` has 208 contiguous cases with real side
 * effects and fallthrough groups, which stays a true jump table.
 * `wide154_sparse_switch` has 200 cases spread on a stride of 4099 across
 * ~800k, which lowers to a binary comparison tree instead.
 *
 * Each selector is reduced modulo slightly more than the case count, so
 * every input lands on a real case (with a small window left over for the
 * default) and the differential exercises the table rather than the
 * fallback. Every call is one table lookup or one tree descent. */

#define WIDE154_DENSE_SPAN 260u
#define WIDE154_EFFECT_SPAN 216u
#define WIDE154_SPARSE_SPAN 205u
#define WIDE154_SPARSE_STRIDE 4099u
#define WIDE154_SLOTS 16

__attribute__((noinline)) int32_t wide154_dense_switch(int32_t selector) {
    int32_t index = (int32_t)((uint32_t)selector % WIDE154_DENSE_SPAN);

    switch (index) {
    case 0: return 1000;
    case 1: return 1007;
    case 2: return 1014;
    case 3: retur