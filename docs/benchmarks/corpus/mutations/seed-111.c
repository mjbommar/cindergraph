#include <stdint.h>

/* Loops with MORE THAN ONE entry point — the graphs no schema matcher can
 * express, and the ones a region-based structurer exists to handle.
 *
 * WHY THIS IS DIFFERENT FROM EVERY OTHER LOOP FIXTURE. `03_loop_shapes`,
 * `12_loop_rotation`, `13_loop_early_exit` and `125_loop_shapes` are all
 * REDUCIBLE: one header dominates the whole body, so `natural_loop_body` finds
 * it from a single back edge. An irreducible loop has two headers and no
 * dominating entry, so `detect_natural_loop` cannot fire at all and
 * `build_full` reaches its `Region::Unstructured` fallback — the lossless
 * whole-function bailout that labels every block.
 *
 * That fallback is correct: `Region::{While,DoWhile}` carry exactly one `exit`,
 * so a multi-entry loop is UNREPRESENTABLE in the region algebra, and inventing
 * a header would move blocks across an edge the machine does not have. The
 * three refusals at the top of `build_full` exist precisely to detect this
 * before a shape can guess. So the fixture's near-term expectation is a
 * faithful goto rendering, and its long-term purpose is to be the acceptance
 * test for the region analysis that replaces the matcher: when `Region` grows
 * owned multi-exits, these functions are how you find out whether it worked.
 *
 * `145_control_flow_flattening` is an OLLVM dispatch loop — flattened, but
 * still reducible, 
eader and one dispatcher. `102_duffs_device`
 * interleaves a switch with a loop but enters at exactly one pla