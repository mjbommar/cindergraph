/* Each initializer completes before the next declarator's initializer. */
static int chain(int x) { int a=x,b=a,c=b; return c; }
static int independent(int x) { int a=0,b=x; return a+b-b; }
static int identity(int x) { return x; }
static int drop(int x) { (void)x; return 0; }
static int calls(int x) {
    int a=identity(x),b=drop(a),c=identity(a);
    return c+b;
}
int main(void) {
    volatile int input = 17;
    if (chain(input) != 17) return 1;
    if (independent(input) != 0) return 2;
    if (calls(input) != 17) return 3;
    return 0;
}
