#include <stdint.h>

/* Aggregates whose members are ALL floating point — the return class `195` left
 * out, and the one where the two ABIs we test disagree the most.
 *
 * `195_by_value_aggregates` covers SysV's INTEGER (`rax`), INTEGER-pair
 * (`rax:rdx`), split-bank (`rax` + `xmm0`) and MEMORY classes. It has no
 * all-SSE case, so nothing in the corpus returns a value in `xmm0:xmm1` — TWO
 * SSE registers holding ONE value, which is a distinct contract from the split
 * case and from a scalar `double`.
 *
 * The same four shapes are a completely different mechanism on AArch64, and the
 * corpus has no lane for that at all: AAPCS64 returns a *homogeneous float
 * aggregate* (2-4 members, all the sam