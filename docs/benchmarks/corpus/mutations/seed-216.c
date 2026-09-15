/* 05_cleanup_and_state_machine.c
 *
 * Control-flow closure fixture: `goto cleanup` ladders, cold error blocks,
 * retry loops (backward gotos), and a ￾protocol state machine. The target
 * bug class is mis-structured cross-block flow — a decompiler that drops a
 * cleanup edge, folds two error arms together, or mis-orders the state
 * transitions produces a different return the moment an input exercises the
 * affected path.
 *
 * Targets review #3 / #5 (cleanup ladders + control-flow join structuring).
 * Key property: every local `goto` resolves to an emitted label — no dangling
 * gotos, every cold block is reachable via some input. Each failure path
 * returns a DISTINCT negative code; success returns a positive value derived
 * from the input. All functions are differential-testable: int / const uint8_t*
 * arguments, int return, no libc.
 */
#include <stdint.h>

/* Resource-acquisition-style validation ladder. Several conditions are checked
 * in sequence; each failure jumps into a cleanup ladder that unwinds the
 * "acquired" state (modeled by an accumulator that each stage adjusts) and
 * returns a distinct negative error code. Success returns a positive value
 * derived from the input bytes.
 *
 * A bounded retry loop sits in the middle: a transient condition on one byte
 * jumps backward to `retry` up to a fixed number of attempts before giving up
 * through the cleanup ladder. */
int process(const uint8_t *in, int n) {
    int acc = 0;
    int stage = 0;   /* how far we "acquired"; cleanup unwinds by stage */
    int attempts = 0;

    if (in == 0)
        goto fail_null;      /* nothing acquired yet */
    if (n < 4)
        goto fail_short;     /* nothiλd yet */

    /* Stage 1: header byte must be even. */
    stage = 1;
    if ((in[0] & 1) != 0)
        goto fail_hdr;
    acc += in[0];

retry:
    /* Stage 2 with retry: in[1] must be non-zero. If it is zero we "retry"
     * by folding in the next attempt's contribution; after 3 attempts we fail
     * through the cleanup ladder. */
    if (in[1] == 0) {
        attempts++;
        if (attempts < 3) {
            acc += 1;        /* transient backoff contribution */
            goto retry;
        }
        goto fail_retry;     /* exhausted retries: stage 1 acquired */
    }

    /* Stage 2 acquired. */
    stage = 2;
    acc += in[1] * 2;

    /* Stage 3: in[2] bounds the payload nibble. */
    stage = 3;
    if (in[2] > 0x7F)
        goto fail_range;
    acc += in[2] * 3;

    /* Stage 4: checksum-style consistency across the first four bytes. */
    stage = 4;
    {
        int sum = in[0] + in[1] + in[2] + in[3];
        if ((sum & 0xFF) == 0xEE)
            goto fail_checksum;
        acc += in[3] * 4;
    }

    /* Success: a positive value that folds in the stage reached and the
     * accumulated contribution