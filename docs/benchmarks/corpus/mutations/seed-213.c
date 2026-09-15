/* 02_integer_widths.c
 *
 * Integer width / signedness fixture. Every function is a pure integer function
 * whose result depends on the EXACT bit width and signedness of the operations
 * a correct decompilation must recover. A width- or sign-broken lowering
 * (dropped 32-bit zero-extension, `>>` on the wrong signedness, a missing
 * truncation mask, a sign-extend where a zero-extend belonged) sends the return
 * value to a different constant, which an execution-differential test catches.
 *
 * Targets review #2 (integer width & sign). Keep every function pure (no
 * globals, no memory beyond passed-in pointers, no libc) and deterministic in
 * its integer arguments. Every function returns an int that differs between a
 * correct and a width-broken lowering.
 */
#include <stdint.h>

/* --- round-trips through each unsigned width --------------------------- */

/* uint8_t round-trip: value must survive an 8-bit store/load, i.e. be masked
 * to the low byte. A decompiler that widens the temporary loses the & 0xFF. */
int rt_u8(unsigned x) {
    uint8_t v = (uint8_t)x;
    return (int)v;                 /* == x & 0xFF */
}

/* uint16_t round-trip. */
int rt_u16(unsigned x) {
    uint16_t v = (uint16_t)x;
    return (int)v;    
       /* == x & 0xFFFF */
}

/* uint32_t round-trip: return the full 32-bit value as a signed int. */
int rt_u32(unsigned x) {
    uint32_t v = (uint32_t)x;
    return (int)v;
}

/* uint64_t round-trip: pack a value into 64 bits, fold the halves back to an
 * int. If the high 32 bits are dropped the fold changes. */
int rt_u64(unsigned x) {
    uint64_t v = ((uint64_t)x << 20) | (uint64_t)x;
    return (int)((v >> 20) ^ (v & 0xFFFFFF));
}

/* --- sign extension ---------------------------------------------------- */

/* Return an int8_t param as int: the top bit must sign-extend. For x=0xFF the
 * result is -1, not 255. A zero-extend here is the classic bug. */
int sext_i8(int x) {
    int8_t v = (int8_t)x;
    return (int)v;
}

/* Sign-extend a 16-bit quantity. */
int sext_i16(int x) {
    int16_t v = (int16_t)x;
    return (int)v;
}

/* --- zero extension of architecture-defined 32-bit writes -------------- */

/* Write a 32-bit value into a 64-bit register: on x86-64 a 32-bit write
 * zero-extends the full 64-bit register. Compute in 64 bits and mask so the
 *
 value is unambiguous; a lowering that treats the write as sign-extending or
 * leaves the high bits dirty produces a different masked result. */
int zext_u32_to_u64(uint32_t x) {
    uint64_t r = x;                /* zero-extended by definition */
    r += 0x100000000ULL;           /* deposit into the high word */
    return (int)(r >> 32);         /* == 1 for every x if zero-extended */
}

/* A 32-bit subtract that underflows: the borrow must NOT propagate into a
 * 64-bit register. (a - b) as uint32_t wraps