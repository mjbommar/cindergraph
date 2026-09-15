#include <stdint.h>

/* A switch wide enough that every target lowers it to a JUMP TABLE rather than
 * a comparison tree, so each architecture's table-dispatch recogniser is
 * exercised by the same source.
 *
 * WHY THIS IS NOT `04_switch_shapes` OR `154_wide_switch`. Those fixtures are
 * about which arms are recovered and whether the exact case constants survive.
 * This one is about whether the DISPATCH ITSELF is recognised, per
 * architecture, and the two failure modes are completely different: an
 * unrecognised table does not produce wrong arms, it produces no arms at all —
 * the indirect branch contributes zero CFG successors and the case bodies never
 * ente