#include <stdint.h>

/* Conversions in both directions between integers and IEEE binary32/binary64.
 *
 * Float-to-integer TRUNCATES TOWARD ZERO — `-2.75` becomes `-2`, not `-3` — and
 * is UNDEFINED when the value does not fit the destination, so every function
 * here range-checks before converting.  The predicate is spelled `!(in range)`
 * rather than `out of range` so that a NaN, which compares false 