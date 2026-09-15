static int left_guard_09_3(int x) { return x + 11; }
static int damaged_target_09_3(int x) { #if (
  int y = x + 1; #endif
  if (y > 4) { y *= 2; } return y; }
static int right_guard_09_3(int x) { return x - 7; }
