/* Textbook: the length check is done in 32 bits and wraps, the copies are not.
 *
 * `hdr + body` is `unsigned int` arithmetic. When the sum wraps past 2^32 the
 * check passes with a tiny `total`, and the two memcpy calls then write `hdr`
 * and `body` bytes — the unwrapped amounts — past the end of `dst`.
 */
#include <stddef.h>
#include <string.h>

// expect: assemble_bug finding
// expect: assemble_fixed clean

// axeyum: capacity(dst) = dst_len
// axeyum: capacity(src) = src_len
int assemble_bug(unsigned char *dst, size_t dst_len, const unsigned char *src, size_t src_len,
                 unsigned int hdr, unsigned int body) {
    unsigned int total = hdr + body;
    if (total > dst_len) return -1;
    if (total > src_len) return -1;
    memcpy(dst, src, hdr);
    memcpy(dst + hdr, src + hdr, body);
    return 0;
}

/* Fixed: do the arithmetic in size_t and check each part before it is used. */
// axeyum: capacity(dst) = dst_len
// axeyum: capacity(src) = src_len
int assemble_fixed(unsigned char *dst, size_t dst_len, const unsigned char *src, size_t src_len,
                   unsigned int hdr, unsigned int body) {
    if (hdr > dst_len || body > dst_len - hdr) return -1;
    if (hdr > src_len || body > src_len - hdr) return -1;
    memcpy(dst, src, hdr);
    memcpy(dst + hdr, src + hdr, body);
    return 0;
}
