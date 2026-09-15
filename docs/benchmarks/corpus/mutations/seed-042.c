#include <stdint.h>

/* A wire codec that packs four fields of different widths into one word and
 * reads them back. The shift/mask constants are the entire specification, so an