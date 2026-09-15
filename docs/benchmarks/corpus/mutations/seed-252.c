/* 12_loop_rotation.c
 *
 * Loop SHAPE fixture. `03_loop_shapes.c` covers what a loop computes; this file
 * covers how the compiler lays one out, because the two disagree across
 * toolchains and our structurer was built against only one of them.
 *
 * gcc -O0 emits a bottom-tested loop: fall into the body, test at the bottom,
 * conditional branch back. clang -O0 emits a ROTATED loop: test at the top with
 * a conditional branch OUT, and an unconditional jump back at the bottom. We
 * structure the first correctly and the second wrongly — `loops.c:factorial` at
 * clang -O0 came out as
 *
 *     while ((n <= 1)) { ...; n = n - 1; goto L; }
 *     return f;
 *     L: ;
 *
 * with the machine's EXIT test used as the CONTINUE condition and the back-edge
 * jumping to a label placed after the return, so the body can never repeat. The
 * functio