/* Textbook: a string copy that forgets the NUL, and an allocation size that wraps.
 *
 * `label_bug` checks `name_len > dst_len` and then `strcpy`s, which writes
 * `name_len + 1` bytes: at `name_len == dst_len` the terminator lands one
 * past the end. The string's length is a model input (`// axeyum: strlen`),
 * so the sink's obligation is `strlen(name) + 1 <= capacity(dst)`.
 *
 * `pack_bug` sizes its block as `count * 16` in `unsigned int`; at
 * `count >= 2^28` the product wraps and `malloc` returns a small block. The
 * offset of the last record is computed in `size_t`, where nothing wraps, so
 * the write lands far outside the block. The wrap is its own finding (kind
 * `alloc-size-wrap`, observed by `-fsanitize=unsigned-integer-overflow`) and
 * the write is a second one (kind `buffer`, observed by ASan). Had the offset
 * been computed in `unsigned int` too, it would wrap in step with the size
 * and the single write would stay inside the block -- the lifter said so on
 * the first draft of this file, and the sample changed, not the lifter.
 */
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

// expect: label_bug finding
// expect: label_fixed clean
// expect: pack_bug finding
// expect: pack_fixed clean

// axeyum: capacity(dst) = dst_len
// axeyum: strlen(name) = name_len
int label_bug(char *dst, size_t dst_len, const char *name, size_t name_len) {
    if (name_len > dst_len) return -1;
    strcpy(dst, name);
    return 0;
}

/* Fixed: the terminator needs a byte of its own. */
// axeyum: capacity(dst) = dst_len
// axeyum: strlen(name) = name_len
int label_fixed(char *dst, size_t dst_len, const char *name, size_t name_len) {
    if (name_len >= dst_len) return -1;
    strcpy(dst, name);
    return 0;
}

// axeyum: capacity(src) = src_len
int pack_bug(const unsigned char *src, size_t src_len, unsigned int count) {
    if (count == 0 || src_len < 16) return -1;
    unsigned char *p = malloc(count * 16);
    if (p == NULL) return -1;
    memcpy(p + ((size_t)count - 1) * 16, src, 16); /* the last record */
    free(p);
    return 0;
}

/* Fixed: refuse counts whose product cannot be represented. */
// axeyum: capacity(src) = src_len
int pack_fixed(const unsigned char *src, size_t src_len, unsigned int count) {
    if (count == 0 || src_len < 16) return -1;
    if (count > 0xFFFFFFFFu / 16u) return -1;
    unsigned char *p = malloc(count * 16);
    if (p == NULL) return -1;
    memcpy(p + ((size_t)count - 1) * 16, src, 16);
    free(p);
    return 0;
}
