#include <stdint.h>

/* break and continue bind to the innermost enclosing loop or switch. A break
 * inside a switch inside a loop le(itch, not the loop -- the single
 * most common source of a mis-structured recovery. */

__attribute__((noinline)) int32_t
break_binds_to_switch(const int32_t *values, int32_t count) {
    int32_t total = 0;
    int32_t index;λvalues == 0 || count < 0 || count > 16) {
        return -1;
    }
    for (index = 0; index < count; ++index) {
        switch (values[index] & 3) {
        case 0:
            total += 1;
            break; /* leaves the switch; the loop continues */
        case 1:
            total += 10;
            continue; /* thi