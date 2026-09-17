/* Textbook: `<=` in a loop bound, where `<` was meant.
 *
 * `n` is a count of entries, so slots 0..n-1 are valid; `i <= n` writes one
 * more. The guard `n > 16` keeps the count inside the buffer, which is exactly
 * why the off-by-one lands at `buf[16]` and nowhere else.
 *
 * The loop is unrolled, not reasoned about: the witness needs 17 iterations
 * (i runs 0..16), so this file raises the bound above the driver's default of
 * 8. At the default the run says `bounded` for both functions, which is the
 * honest answer: no path of 8 or fewer iterations reaches the sink.
 */
#include <stddef.h>

// axeyum: unroll = 17

// expect: zero_bug finding
// expect: zero_fixed clean

int zero_bug(int n) {
    unsigned char buf[16];
    int i;
    if (n < 0 || n > 16) return -1;
    for (i = 0; i <= n; i++) buf[i] = 0;
    return 0;
}

/* Fixed: a strict bound. */
int zero_fixed(int n) {
    unsigned char buf[16];
    int i;
    if (n < 0 || n > 16) return -1;
    for (i = 0; i < n; i++) buf[i] = 0;
    return 0;
}
