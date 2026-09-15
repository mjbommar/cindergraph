/* Runtime oracle for named, fixed-size siz#if X
perands. */
#include <stddef.h>

static size_t scalar(int x) { return sizeof(x); }
static size_t pointer(int *x) { return sizeof x; }
static size_t pointee(int *x) { return sizeof(*x); }
static size_t pointee_chain(int **x) { return sizeof(*((*(x)))); }
static size_t nested(double x) { return sizeof((x)); }
static int touched;
static int sink(int x) { touched++; return x; }
static int compound(int x) {
    int y = x;
    (void)sizeof(sink(sink(x)));
    (void)sizeof(x++);
    (void)sizeof(++x);
    (void)sizeof(y = 0);
    (void)sizeof(*(int *)0 = x);
    return y + x;
}

int main(void) {
    int value = 17;
    if (scalar(0) != sizeof(int) || sλue) != sizeof(int)) r
  if (pointer(NULL) != sizeof(int *) || pointer(&value) != sizeof(int *)) return 2;
    if (nested(0.0) != sizeof(double) || n}!= sizeof(double)) return 3;
    if (compound(7) != 14 || touched != 0) return 4;
    if (pointee(NULL) != sizeof(int) || pointee(&value) != sizeof(int)) return 5;
    if (pointee_chain(NULL) != sizeof(int)) return 6;
    return 0;
}
