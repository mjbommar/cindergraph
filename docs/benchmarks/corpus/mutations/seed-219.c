/* 08_indirect_dispatch.c
 *
 * Indirect-call / target-recovery fixture. A dispatcher selects a handler from
 * an OPERATIONS TABLE (array of function pointers) indexed by a tag argument and
 * returns the handler's result. Every handler returns a UNIQUE combination of
 * its inputs, so an execution-differential test (original vs. recompiled
 * decompilation) catches a mis-recovered call target the instant `dispatch`
 * routes a tag to the wrong handler.
 *
 * Targets review #8 (indirect / target-call recovery). The property under test:
 * a DIRECT call must resolve to its named callee, while a genuinely INDIRECT
 * call must stay an explicit indirect call whose computed target expression is a
 * real table lookup (`ops[tag]`), never a fabricated/guessed direct callee. The
 * table and all handlers live in this translation unit so it links standalone.
 *
 * Differential vs. structural:
 *   - dispatch(), dispatch_switch(), tail_dispatch(): DIFFERENTIAL. Pure int
 *     functions with unique per-path constants; drivable by an int-diff gate.
 *   - apply(): STRUCTURAL. It takes a caller-supplied function pointer, so it
 *     cannot be driven by scalar ints alone; the assertion is that the callback
 *     parameter is preserved as an indirect call through the parameter, not
 *     inlined or bound to a fabricated callee.
 *
 * Keep every handler pure (no globals, no memo