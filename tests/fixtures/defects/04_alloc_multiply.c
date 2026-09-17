/* Textbook: an allocation size computed in 32 bits from a 32-bit count.
 *
 * `count * 16u` wraps when `count >= 2^28`; the check against `dst_len` then
 * passes with a small `bytes`, but the caller believes `count` records fit.
 * Here the record loop is written out as the first record's write so the
 * function stays loop-free: the sink is that record `count - 1` lies outside
 * `bytes`.
 */
#include <stddef.h>
#include <string.h>

// expect: fill_records_bug finding
// expect: fill_records_fixed clean

// axeyum: capacity(dst) = dst_len
// axeyum: capacity(src) = src_len
int fill_records_bug(unsigned char *dst, size_t dst_len, const unsigned char *src, size_t src_len,
                     unsigned int count) {
    unsigned int bytes = count * 16u;
    if (bytes > dst_len) return -1;
    if (bytes > src_len) return -1;
    if (count == 0) return 0;
    /* last record: offset (count - 1) * 16, length 16 */
    memcpy(dst + (count - 1) * 16u, src, 16);
    return 0;
}

/* Fixed: refuse counts whose product cannot be represented. */
// axeyum: capacity(dst) = dst_len
// axeyum: capacity(src) = src_len
int fill_records_fixed(unsigned char *dst, size_t dst_len, const unsigned char *src, size_t src_len,
                       unsigned int count) {
    if (count > 0xFFFFFFFFu / 16u) return -1;
    unsigned int bytes = count * 16u;
    if (bytes > dst_len) return -1;
    if (bytes > src_len) return -1;
    if (count == 0) return 0;
    memcpy(dst + (count - 1) * 16u, src, 16);
    return 0;
}
