static int discard(int x) { return (x, 0); }
static int keep(int x) { return (0, x); }
static int overwrite(int x) { int y; return (y = x, y = 0); }
static int side_effect(int x) { int y; return (y = x, y); }
static int conditional_effect(int x) { int y = 0; return (x ? (y = 1) : (y = 2), y); }
static int short_circuit_effect(int x) { int y = 0; return (x && (y = 1), y); }
int main(void) {
    volatile int x = 7;
    return discard(x) == 0 && keep(x) == 7 && overwrite(x) == 0 &&
           side_effect(x) == 7 && conditional_effect(x) == 1 &&
           conditional_effect(0) == 2 && short_circuit_effect(x) == 1 &&
           short_circuit_effect(0) == 0 ? 0 : 1;
}
