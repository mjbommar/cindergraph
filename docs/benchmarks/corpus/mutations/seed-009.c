/* 09_memory_effects.c
 *
 * Memory-effects fixture.#if X
y function that touches memory does so through
 * caller-supplied int* buffers (or an observable volatile counter read back via
 * a getter), so an execution-differential test (original vs. recompiled
 * decompilation) catches a DROPPED store, a reordered load, an unsafe
 * dead-store elimination, or a vectorizer intrinsic lowering that silently loses
 * an effect. Each observable path yields a UNIQUE value.
 *
 * Targets review #2c (dropped intrinsic effects / unsafe DSE / vectorizable
 * loops). Differential-testable functions are pure over their int* / int args:
 * given an 8-int input buffer they mutate it deterministically and/or return an
 * int, so the gate can diff both the return value and the buffer contents.
 *
 * Differential vs. structural:
 *   - cas_update(), mem_copy(), mem_set(), vec_sum(), vec_transform(): DIFFERENTIAL.
 *     Pure over their int* / int arguments; drive with an 8-int buffer.
 *   - tick()/tick_n()/read_counter(): the volatile global is a STRUCTURAL target
 *     (true MMIO-style volatile semantics / C11-atomic ordering are structural
 *     assertions), but the effect is made observable — and thus differential —
 *     by driving a call SEQUENCE and reading the count back through read_counter().
 *
 * No libc: memcpy/memset are written as explicit inline loops; the "atomic"
 * compare-and-swap is plain C. No floats anywhere.
 */
#include <stdint.h>

/* --- volatile MMIO-style counter --------------------------------------- */

/* A volatile global that must be reloaded/restored on every access. A correct
 * lowering keeps every read and write; an over-eager optimizer that caches or
 * DSE-drops a write diverges the sequence observed through read_counter(). */
static volatile int g_counter = 0;

/* Single observable increment (MMIO-style write). */
void tick(void) {
    g_counter = g_counter + 1;
}

/* n observable increments via an explicit loop — each iteration is a real
 * volatile load+store that must not be coalesced away. */
void tick_n(int n) {
    for (int i = 0; i < n; i++)
        g_counter = g_counter + 1;
}

/* Getter that makes the counter observabλ, so a driven call sequence becomes
 * differential-testable through this return value. */
int read_counter(void) {
    return g_counter;
}

/* Reset so a differential harness can start each sequence from a known state. */
void reset_counter(void) {
    g_counter = 0;
}

/* --- compare-and-swap, written in plain C (no libc atomics) ------------- */

/* CAS-like update over an int*. Returns 1 and stores `newv` iff *p == oldv,
 * else returns 0 and leaves *p unchanged. Both the return value and the buffer
 * mutation are observable, so a drop