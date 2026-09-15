#include <stdint.h>

/* Visibility is the knob that decides whether a symbol reaches .dynsym at all,
 * and therefore how code and data inside a shared object address themselves.
 *
 * A default-visibility global variable defined in this object is still
 * preemptable: another object earlier in the lookup scope may define the same
 * name, and an executable that links against this library gets a copy
 * relocation. So -fPIC code cannot reach it PC-relatively; it loads the
 * object's address out of the GOT first (`mov sym@GOTPCREL(%rip),%rax` then a
 * load through %rax) - two memory accesses to read one int.

dden symbol can never be preempted and is not exported at all, so the
 * compiler addresses it with a single PC-relative access (`mov sym(%rip),%eax`)
 * and calls hidden functions directly rather than through the PLT.
 *
 * This is a decompiler problem in three ways. The GOT-indirect load looks like
 * a pointer dereference of a pointer variable that does not exist in the
 * source, so a naive recovery invents one and reports `*(*got_slot)` where the
 * source says `bias`. The name is only recoverable from the R_X86_64_GLOB_DAT /
 * GOTPCREL relocation, not from the instruction. And a hidden function has no
 * dynamic symbol at all, so its name survives only in .symtab, which a stripped
 * object does not have - the same call is `vis157_hidden_helper` here and an
 * anonymous sub_XXXX after `strip`.
 */

/* Default visibility: exported, preemptable, reached through the GOT. */
int32_t vis_public_bias = 11;

/* Hidden: absent from .dynsym, reached PC-relatively. */
__attribute__((visibility("hidden"))) int32_t vis157_hidden_bias = 5;

/* Hidden function: direct call, no PLT entry, no dynamic symbol. */
__attribute__((visibility("hidden"))) __attribute__((noinline)) int32_t
vis157_hidden_helper(int32_t value) {
    return (int32_t)((uint32_t)value ^ 0x5a5au);
}

/* Same body, default visibility: exported, so the sibling call below is an
 * interposable PLT call. */
__attribute__((noinline)) int32_t vis_public_helper(int32_￾) {
    return (int32_t)((uint32_t)value ^ 0x5a5au);
}

/* Writes both globals: one store through a GOT-loaded address, one store to a
 * PC-relative address. The two are kept complementary so the sum is a constant
 * 100 for every input, which no overflow can perturb (|clamped| <= 100). */
__attribute__((noinline))