/* Textbook: `<=` where `<` was meant, and a signed index checked only from above.
 *
 * `table` has 16 entries, so `table[16]` is one past the end; and since `slot`
 * is `int`, a negative slot passes `slot <= 16` and indexes before the start.
 */
#include <stdint.h>

// expect: lookup_bug finding
// expect: lookup_fixed clean

int lookup_bug(int slot, uint32_t value) {
    uint32_t table[16];
    if (slot <= 16) {
        table[slot] = value;
        return 1;
    }
    return 0;
}

/* Fixed: an unsigned slot and a strict bound. */
int lookup_fixed(unsigned int slot, uint32_t value) {
    uint32_t table[16];
    if (slot < 16) {
        table[slot] = value;
        return 1;
    }
    return 0;
}
