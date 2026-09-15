#include <stdint.h>

/* A byte-level lexer driven by an explicit state variable.  Character
 * classification produces a dense switch over a small domain, and the accepted
 * token count is written through an output pointer, so both the jump table and
 * the store-through-parameter must survive. */

#define TOK_MAX 16

#define TOK_STATE_BLANK 