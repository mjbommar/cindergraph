/* Textbook: a length is narrowed before it is checked.
 *
 * `size_t n` is stored into `unsigned short len` (low 16 bits), the check
 * then bounds `len`, and memcpy uses the original `n`.
 */
#include <stddef.h>
#include <string.h>

// expect: store_bug finding
// expect: store_fixed clean

// axeyum: capacity(dst) = 4096
// axeyum: capacity(src) = src_len
int store_bug(unsigned char *dst, const unsigned char *src, size_t src_len, size_t n) {
    unsigned short len = n;
    if (len > 4096) return -1;
    if (n > src_len) return -1;
    memcpy(dst, src, n);
    return 0;
}

// axeyum: capacity(dst) = 4096
// axeyum: capacity(src) = src_len
int store_fixed(unsigned char *dst, const unsigned char *src, size_t src_len, size_t n) {
    if (n > 4096) return -1;
    if (n > src_len) return -1;
    memcpy(dst, src, n);
    return 0;
}
