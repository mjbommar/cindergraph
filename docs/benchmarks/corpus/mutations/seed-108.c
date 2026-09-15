#include <stdint.h>

/* A chain of `if (c) { handler; }` guards whose handlers the compiler moves OUT
 * OF LINE, so each one sits after the function's tail and jumps back.
 *
 * THE LARGEST MEASURED DEFECT CLASS. Over the 250 scored DecBench sample-set
 * functions, 28.8% render as goto soup (40.5% on x86-64). A census of those
 * splits them by how much of the function was lost: only 2.6% are whole-
 * function bailouts, so `build_full`'s three refusals are NOT the cause. The
 * loss is `detect_if_shape` declining shape by shape and taking the remainder
 * of the walk with it — one conditional fails to match, and every block after
 * it lands in `Region::Unstructured`, which the renderer emits as one label per
 * block.
 *
 * `bin_090.elf sub_7370` in the frozen sample-set is the smallest instance: 15
 * blocks, 12 of them labelled. This fixture is that shape in C.
 *
 * WHY EXECUTION CANNOT SEE IT. Goto soup is FAITHFUL. Every arm is present,
 * every edge is real, the C compiles and returns the right answer for every
 * input. It is simply not the s￾trol flow. That is why this fixture
 * carries `goto_free` and `switch` structural assertions rather than relying on
 * the execution differential — before those predicates existed the corpus had
 * no way to state the property at all.
 *
 * NOT `105_goto_ladder` (which is ABOUT goto, in the source), and not
 * `107_short_circuit` (which is about operand evaluation order). The source
 * here contains no goto whatsoever.
 *
 * `__builtin_expect` marks the handlers cold so gcc and clang both sink them
 * below the return. The `cold` attribute would be stronger but is not availabl