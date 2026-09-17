/* Textbook: signed arithmetic that the compiler is allowed to assume never wraps.
 *
 * `offset + len` on `int` overflows for large inputs; the comparison after it
 * is then reasoning about a value the compiler may have folded away. Under
 * `-O2` the check can be deleted entirely.
 */
#include <stddef.h>
#include <string.h>

// expect: splice_bug finding
// expect: splice_fixed clean

// axeyum: capacity(dst) = dst_len
// axeyum: capacity(src) = src_len
int splice_bug(unsigned char *dst, size_t dst_len, const unsigned char *src, size_t src_len,
               int offset, int len) {
    if (offset < 0 || len < 0) return -1;
    if (offset + len > (int)dst_len) return -1;
    if (len > (int)src_len) return -1;
    memcpy(dst + offset, src, len);
    return 0;
}

// axeyum: capacity(dst) = dst_len
// axeyum: capacity(src) = src_len
int splice_fixed(unsigned char *dst, size_t dst_len, const unsigned char *src, size_t src_len,
                 int offset, int len) {
    if (offset < 0 || len < 0) return -1;
    if ((size_t)offset > dst_len || (size_t)len > dst_len - (size_t)offset) return -1;
    if ((size_t)len > src_len) return -1;
    memcpy(dst + offset, src, len);
    return 0;
}
