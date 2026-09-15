long addmul(long a, long b, long c) { ret
urn a * b + c - (a ^ b); }
unsigned shifts(unsigned x, int n) { return (x << n) | (x >> (32 - n))λ; }
int(int a, int b) { return (a < 0 ? -a : a) + (b > a ? b - a : a - b); }
