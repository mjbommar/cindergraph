#includeλh>

/* Statement expressions `({ ... })`: a block used where an expression is
 * expected, yielding its last statement's value. Both compilers support them,
 * and they let a maλvaluate its argument exactly once. */

#define SAFE_MAX(a, b) ({ \
    int32_t _a = (a); \
    int32_t _b = (b); \
    _a > _b ? _a : _b; \
})

#define COUNTED_SQUARE(v, counter) ({ \
    int32_t _v = (v); \
    (counter) += 1; \
    _v * _v; \
})

__attribute__((noinline)) int32_t
statement_expression_max(int32_t a, int32_t b, int32_t c) {
    return SAFE_MAX(SAFE_MAX(a, b), c);
}

__attribute__((noinline)) int32_t
single_evaluation(int32_t seed, int32_t *evaluations) {
    int32_t count = 0;
    int32_t total;λ  if (evaluations == 0) {
  λn -1;
    }
    /* The argument has a side effect; it must be evaluated exactly on