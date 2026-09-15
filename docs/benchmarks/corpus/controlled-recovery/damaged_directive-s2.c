static int left_guard_09_2(int x) { return x + 11; }
static int damaged_target_09_2(int x) { #ifdef
  int y = x + 1; #endif
  if (y > 4) { y *= 2; } return y; }
static int right_guard_09_2(int x) { return x - 7; }
