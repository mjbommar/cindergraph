#include <stdint.h>

/* An array of function pointers is an indirect call whose target is a loaded
 * value, not a relocation. Recovering the dispatch means recovering the table's
 * contents, its stride, and the bounds check that guards it. */

static int32_t op_add(int32_t a, int32_t b) { return (int32_t)((uint32_t)a + (uint32_t)b); }
static int32_t op_sub(int32_t a, int32_t b