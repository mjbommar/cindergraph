#include <stdint.h>

__attribute__((noinline)) int32_t topological_sort(const int32_t *adjacency,
                                                    int32_t n,
                                                    int32_t *o