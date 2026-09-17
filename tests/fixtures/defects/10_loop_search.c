/* Textbook: a bounded linear search whose "not found" exit is one past the end.
 *
 * The loop looks for the first of 8 slots whose id (`4 * i`, computed rather
 * than stored, so the search does not depend on memory the model leaves
 * unconstrained) reaches `wanted`. When none does, the loop exits normally
 * with `i == 8` and `taken[i]` reads past the array. Eight iterations fit the
 * default bound exactly, so the run also proves the loop never needs a ninth:
 * the fixed twin is `clean`, not merely `bounded`.
 */
#include <stdint.h>

// expect: find_slot_bug finding
// expect: find_slot_fixed clean

int find_slot_bug(unsigned int wanted) {
    unsigned char taken[8];
    unsigned int i;
    for (i = 0; i < 8; i++) {
        if (4 * i >= wanted) break;
    }
    return taken[i];
}

/* Fixed: the not-found exit is checked before the read. */
int find_slot_fixed(unsigned int wanted) {
    unsigned char taken[8];
    unsigned int i;
    for (i = 0; i < 8; i++) {
        if (4 * i >= wanted) break;
    }
    if (i == 8) return -1;
    return taken[i];
}
