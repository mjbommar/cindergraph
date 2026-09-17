/* Textbook: a local assigned on some branches and read on all of them.
 *
 * `flags` is set when `mode` is 1 or 2 and read unconditionally; for any
 * other `mode` the read sees whatever was on the stack. The lifter tracks,
 * per path, which locals no assignment has reached yet, so the read is a
 * finding exactly on the `mode` values that skip both assignments.
 *
 * MemorySanitizer observes an uninitialised value when it decides a branch,
 * which is why the read here is a condition: a copy into a return value
 * would be invisible to it, and the replay would then say so rather than
 * count the finding.
 */

// expect: parse_mode_bug finding
// expect: parse_mode_fixed clean

int parse_mode_bug(int mode) {
    int flags;
    if (mode == 1) flags = 4;
    else if (mode == 2) flags = 8;
    if (flags & 4) return 1;
    return 0;
}

/* Fixed: a default before the branches. */
int parse_mode_fixed(int mode) {
    int flags = 0;
    if (mode == 1) flags = 4;
    else if (mode == 2) flags = 8;
    if (flags & 4) return 1;
    return 0;
}
