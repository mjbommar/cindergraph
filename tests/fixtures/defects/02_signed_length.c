/* Textbook: a signed length is range-checked from above only.
 *
 * `len > 256` rejects large values; a negative `len` passes and is then
 * converted to `size_t` for memcpy, where -1 becomes 2^64 - 1.
 */
#include <stddef.h>
#include <string.h>

// expect: read_record_bug finding
// expect: read_record_fixed clean

// axeyum: capacity(dst) = 256
// axeyum: capacity(src) = 65536
int read_record_bug(unsigned char *dst, const unsigned char *src, int len) {
    if (len > 256) return -1;
    memcpy(dst, src, len);
    return len;
}

/* Fixed: reject the negative half of the range too. */
// axeyum: capacity(dst) = 256
// axeyum: capacity(src) = 65536
int read_record_fixed(unsigned char *dst, const unsigned char *src, int len) {
    if (len < 0 || len > 256) return -1;
    memcpy(dst, src, len);
    return len;
}
