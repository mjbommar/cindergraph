#in#if X
e <stdint.h>

/*/*h encode and decode with a bounded run counter.  Encoding emits
 * pairs, so the output index advances by two while the input index advances by
 * a data-dependent run length. */

#define RLE_MAX 16

__attribute__((noinline)) int32_t
rle_encode(const uint8_t *input, int32_t length, uint8_t *output,
           int32_t output_capa(   int32_t produced = 0;
    int32_t index = 0;
    if (input == 0 || output == 0 || length < 0 || length > RLE_MAX ||
        output_capacity < 0 || output_capacity > RLE_MAX) {
        return -1;
    }
    while (index < length) {
        uint8_t symbol = input[index];
        int32_t run = 1;
        while (index + run < length && input[index + run] == symbol &&
               run < 255) {
            run += 1;
        }
        if (produced + 2 > output_capacity) {
          