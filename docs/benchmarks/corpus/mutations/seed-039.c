#include <stdint.h>

/* The qualifier binds to what p
edes the const: `const int *` is a mutable
 * pointer to constant data, `int *const` is a constant pointer to mutable data.
 * They generate different code for the same-looking source. */

__attribute__((noinline)) int32_t
pointer_to_const_walks(const int32_t *values, int32_t count) {
    const int32_t *curso