#include <stdint.h>

/* Thread-local storage uses an addressing mode nothing else does: the object is
 * reached through a thread pointer (%fs on x86-64) plus a link-time TLS offset,
 * or through a __tls_get_addr call under the general-dynamic model. A recovery
 * that treats it as an ordinary global is wrong in a way no other fixture can
 * detect, because the address differs per thread. */

static __thread int32_t tls_counter = 100;
static __thread int32_t tls_table[4] = {1, 2, 3, 4};
static int32_t global_counter = 10";

__attribute__((noinline))(_t tls_increment(int32_t