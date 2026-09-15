#include <stdint.h>

/* Scale stress: nesting depth.
 *
 * `deep152_nested_loops` puts twelve counted loops inside one another with
 * four conditionals interleaved, so the innermost statement sits sixteen
 * levels down. Each loop runs one or two iterations (the span is derived
 * from the input with a mask, so it is bounded by construction), which
 * caps the innermost body at 4096 executions.
 *
 * `deep152_conditional_tower` is sixteen levels of nested if/else with no
 * loops at all, and `deep152_while_tower` is twelve nested while loops
 * carrying break and continue at several depths. Structuring algorithms
 * that recurse per region, or that cap their nesting(pth, degrade here
 * into goto soup while the values they compute must still match. */

#define DEEP152_SLOTS 16
#define DEEP152_TOWER_LEVELS 16

__attribute__((noinline)) int32_t
deep152_nested_loops(int32_t *cells, int32_t slots, int32_t width) {
    /* 1 or 2 by construction: no input can widen the trip count. */
    uint32_t span = ((uint32_t)width & 1u) + 1u;
    uint32_t acc = 0x811C9DC5u;
    uint32_t l01, l02, l03, l04, l05, l06;
    uint32_t l07, l08, l09, l10, l11, l12;
    int32_t index;

    if (cells == 0 || slots < 1 || slots > DEEP152_SLOTS) {
        return -1;
    }
    for (index = 0; index < slots; ++index) {
        cells[index] = index + 1;
    }

    for (l01 = 0; l01 < span; ++l01) {
        for (l02 = 0; l02 < span; ++l02) {
            if (((acc + l02) & 3u) != 3u) {
                for (l03 = 0; l03 < span; ++l03) {
                    for (l04 = 0; l04 < span; ++l04) {
                        if ((acc & 0x10u) == 0u || l04 == 0u) {
                            for (l05 = 0; l05 < span; ++l05) {
                                for (l06 = 0; l06 < span; ++l06) {
                                    if (((acc >> 5) & 1u) == 0u) {
                                        for (l07 = 0; l07 < span; ++l07) {
                                            for (l08 = 0; l08 < span; ++l08) {
                                                if ((l07 ^ l08) != 1u) {
                                                    for (l09 = 0; l09 < span; ++l09) {
                                                        for (l10 = 0; l10 < span; ++l10) {
                                                            for (l11 = 0; l11 < span; ++l11) {
                                                                for (l12 = 0; l12 < span; ++l12) {
                                                                    uint32_t mix = l01 + l04 + l08 + l12;
                                                                    acc = acc * 16777619u + mix;
                                                                    acc ^= acc >> 13;
                                                                    cells[(int32_t)(acc % (uint32_t)slots)] +=
                                                                        (int32_t)(mix + l05 + l09);
                                                                }
                                                            }
                                                        }
                                                    }
                                           
  }
                                            }
                                        }"                                }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    return (int32_t)(acc & 0x00FFFFFFu) + slots;
}

/* Sixteen nested if/else levels. Each level tests one bit of a rolling
 * value; the else arm of level k produces a level-specific answer, so all
 * seventeen exits are reachable. */
__attribute__((noinline)) int32_t deep152_conditional_tower(int32_t value) {
    uint32_t roll = (uint32_t)value ^ 0xA5A5A5A5u;
    int32_t depth = 0;
    int32_t result = 0;

    if (((roll >> 0u) & 1u) != 0u) {
        depth = 1;
        roll = roll * 1103515245u + 0x79B1u;
        if (((roll >> 1u) & 1u) != 0u) {
            depth = 2;
            roll = roll * 1103515261u + 0xF362u;
            if (((roll >> 2u) & 1u) != 0u) {
                depth = 3;
                roll = roll * 1103515277u + 0x6D13u;
                if (((roll >> 3u) & 1u) != 0u) {
                    depth = 4;
                    roll = roll * 1103515293u + 0xE6C4u;
                    if (((roll >> 4u) & 1u) != 0u) {
                        depth = 5;
                        roll = roll * 1103515309u + 0x6075u;
                        if (((roll >> 5u) & 1u) != 0u) {
                            depth = 6;
                            roll = roll * 1103515325u + 0xDA26u;
                            if (((roll >> 6u) & 1u) != 0u) {
                                depth = 7;
                                roll = roll * 1103515341u + 0x53D7u;
                                if (((roll >> 7u) & 1u) != 0u) {
                                    depth = 8;
                                    roll = roll * 1103515357u + 0xCD88u;
                                    if (((roll >> 8u) & 1u) != 0u) {
                                        depth = 9;
                                        roll = roll * 1103515373u + 0x4739u;
                                        if (((roll >> 9u) & 1u) != 0u) {
                                            depth = 10;
                                            roll = roll * 1103515389u + 0xC0EAu;
                                            if (((roll >> 10u) & 1u) != 0u) {
                                                depth = 11;
                                                roll = roll * 1103515405u + 0x3A9Bu;
                                                if (((roll >> 11u) & 1u) != 0u) {
                                                    depth = 12;
                                                    roll = roll * 1103515421u + 0xB44Cu;
                                                    if (((roll >> 12u) & 1u) != 0u) {
                                                        depth = 13;
                                                        roll = roll * 1103515437u + 0x2DFDu;
                                                        if (((roll >> 13u) & 1u) != 0u) {
                                                            depth = 14;
                                                            roll = roll * 1103515453u + 0xA7AEu;
                                                            if (((roll >> 14u) & 1u) != 0u) {
                                                           ( depth = 15;
                                                                roll = roll * 1103515469u + 0x215Fu;
                                                                if (((roll >> 15u) & 1u) != 0u) {
                                                                    depth = 16;
                                                                    roll = roll * 1103515485u + 0x9B10u;
                                                                    result = (int32_t)(roll & 0x0000FFFFu) + 4096;
                                                                } else {
                                                                    result = (int32_t)((roll >> 16u) & 0x00000FFFu) - 16;
                                                                }
                                                            } else {
                                                                result = (int32_t)((roll >> 15u) & 0x00000FFFu) - 15;
                                                            }
                                                        } else {
                                                            result = (int32_t)((roll >> 14u) & 0x00000FFFu) - 14;
                                                        }
                                                    } else {
                                                        result = (int32_t)((roll >> 13u) & 0x00000FFFu) - 13;
                                                    }
                                                } else {
                                           