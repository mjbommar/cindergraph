/* A by-value parameter's address names callee-local storage. */
static void replace_scalar(int x) {
    int *p = &x;
    *p = 0;
}

static void replace_pointer(int *p) {
    int **q = &p;
    *q = 0;
}

int main(void) {
    int x = 7;
    int *p = &x;
    replace_scalar(x);
    replace_pointer(p);
    return x == 7 && p == &x ? 0 : 1;
}
