/* 162_unaligned_memcpy_access.c
 *
 * The portable unaligned-access idiom: to read or write a multi-byte scalar at
 * an arbitrary byte offset in a wire buffer you do NOT cast the byte pointer to
 * `uint32_t *` (that forms a misaligned pointer -- undefined even if you never
 * dereference it -- and violates the effective-type rules); you `memcpy` into
 * or out of a properly aligned local of the target type. Every serious protocol
 * and firmware codebase is written this way, and every compiler recognises the
 * fixed-size form and emits a single unaligned load or store.
 *
 * Why this stresses a decompiler: that recognition is exactly what erases the
 * idiom. At -O2 a 4-byte `memcpy` into a local becomes one `mov`, a 2-byte one
 * becomes a `movzx`, and there is no call, no loop, and no local left to see.
 * At -O0 the same source keeps a real `memcpy` call against a stack slot. So
 * the SAME line of C has two completely different shapes across the optimisation
 * lanes, and the decompiler has to land on the same semantics from both: the
 * width of the access, the byte offset it happens at, and whether the value is
 * reassembled in host order or in a declared wire order. Getting the width right
 * and the offset off by one still recompiles, still type-checks, and decodes a
 * different field.
 *
 * The native-order accessors below are host-endian by construction and are
 * documented as such; the big-endian accessor copies into a local byte array
 * first and reassembles with shifts, so it is endianness-independent. Both are
 * real idioms and a parser normally contains both.
 *
 * UB notes: every offset is validated against a length that is clamped to
 * [0, 16] BEFORE any subtraction, so a hostile INT_MIN length can never
 * overflow. No pointer is ever cast to a wider scalar type.
 */
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#define UA162_MAX_BUF 16
#define UA162_BAD32   0xFFFFFFFFu

/* Clamp a caller length into [0, UA162_MAX_BUF] before it is used in any
 * arithmetic, so `limit - 4` below is always well defined. */
static int32_t ua162_clamp(int32_t n) {
    if (n < 0) {
        return 0;
    }
    if (n > UA162_MAX_BUF) {
        return UA162_MAX_BUF;
    }
    return n;
}

/* 1 iff [off, off + width) lies inside `limit` bytes. `limit` is pre-clamped
 * and `width` is a small constant, so no subtraction here can overflow. */
static int32_t ua162_fits(int32_t limit, int32_t off, int32_t width) {
    if (off < 0) {
        return 0;
    }
    return (off <= limit - width) ? 1 : 0;
}

/* Unaligned 4-byte load in HOST byte order -- the canonical idiom. Returns
 * 0xFFFFFFFF on rejection (ambiguous with that literal payload value, which is
 * why real code returns a status; this fixture returns the value directly so
 * the differential compares the full 32 bits). */
__attribute__((noinline)) uint32_t
ua162_load_native32(const uint8_t *buf, int32_t len, int32_t off) {
    uint32_t value = 0;
    int32_t limit = ua162_clamp(len);

    if (buf == NULL) {
        return UA162_BAD32;
    }
    if (!ua162_fits(limit, off, 4)) {
        return UA162_BAD32;
    }
    memcpy(&value, buf + off, sizeof value);
    return value;
}

/* Unaligned 2-byte load in HOST byte order. A uint16_t can never be 0xFFFFFFFF,
 * so the sentinel is unambiguous here. */
__attribute__((noinline)) uint32_t
ua162_load_native16(const uint8_t *buf, int32_t len, int32_t off) {
    uint16_t value = 0;
    int32_t limit = ua162_clamp(len);

    if (buf == NULL) {
        return UA162_BAD32;
    }
    if (!ua162_fits(limit, off, 2)) {
        return UA162_BAD32;
    }
    memcpy(&value, buf + off, sizeof value);
    return (uint32_t)value;
}

/* Unaligned 4-byte load in declared BIG-endian wire order: copy the bytes into
 * an aligned local array