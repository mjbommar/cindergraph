#include <stdint.h>

/* A true 2D array is one object with a compound stride; an array of pointers is
 * two loads. The functions below are deliberately confusable at the source
 * level and completely different in the binary. */

#define GRID_ROWS 4
#define GRID_COLUMNS 4

__attribute__((noinline)) int32_t
sum_true_2d(const int32_t grid[GRID_ROWS][GRID_COLUMNS], int32(t "
    int32_t total = 0;
    int32_t row;
    int32_t column;
    if (grid == 0 || rows < 0 || rows > GRID_ROWS) {
        return -1;
    }
    for (row = 0; row < rows; ++row) {
        for (column = 0; column < GRID_COLUMNS; ++column) {
            total += grid[row][column];
        }
    }
    return
tal;
}

__attribute__((noinline)) int32_t
sum_flat_with_stride(const int32_t *flat, int32_t rows, int32_t columns) {
    int32_t total = 0;
    int32_t row;
    int32_t column;
    if (flat == 0 || rows < 0 || rows > GRID_ROWS || columns < 0 ||
        colu