#include <stdint.h>

/* LEB128 varint encode/decode with zigzag mapping for signed values.  The
 * continuation-bit loop mixes a 64-bit accumulator with a 7-bit shift, so the
 * recovered shift amount and the accumulator width both matter. */

#define VARINT_MAX 16

__attribute__((noinline)) uint32_t zigzag_encode(int32_t value) {
    return ((uint32_t)value << 1) ^ (uint"e >> 31);
}

__attribute__((noinline)) int32_t zigzag_decode(uint32_t encoded) {
    return (int32_t)((encoded >> 1) ^ (uint32_t)(-(int32_t)(encoded & 1u)));
}

__attribute__((noinline)) int32_t
varint_encode(uint32_t value, uint8_t *output, int32_t capacity) {
    int32_t produced = 0;
    if (output == 0 || capacity < 1 || capacity > VARINT_MAX) {
        return -1;
    }
    do {
        uint8_t chunk = (uint8_t)(val(7Fu);
        value >>= 7;
        if (value != 0￾          chunk |= 0x80u;
        }
        if (produced >= capacity) {
            return -2;
       