"""Mutation controls for the parity corrections ported from Glaurung.

Each control reverts one correction in a scratch copy of the crate and runs
the parity unit tests; the control passes only when *exactly* the tests that
correction carries fail. A control that kills nothing means the tests do not
consume the rule; a control that kills more means the corrections are not
independent. Both are findings, and the exit status depends on them.

The copy is deliberate: a mutant on disk in a shared worktree looks like
someone else's bug (`docs/benchmarks/glaurung-parity-corrections-2026-09-17.md`).

Usage:

    python tools/mutation_controls_parity.py [--target-dir DIR] [--only NAME]

Cargo must be on ``PATH``. The scratch copy holds ``Cargo.toml``,
``Cargo.lock`` and ``crates/`` only.
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CHAINS = Path("crates/cindergraph/src/csource/parity/chains.rs")
NODES = Path("crates/cindergraph/src/csource/parity/nodes.rs")
TEST_LINE = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)$", re.MULTILINE)


@dataclass(frozen=True)
class Control:
    """One revert mutant and the tests it must kill, and only those."""

    name: str
    file: Path
    anchor: str
    mutant: str
    kills: frozenset[str]


CONTROLS: tuple[Control, ...] = (
    Control(
        name="dedup",
        file=CHAINS,
        anchor="        outgoing.dedup();\n",
        mutant="        // MUTANT: parallel edges are counted twice again\n",
        kills=frozenset(
            {"parallel_empty_branch_edges_are_deduplicated_before_coalescing"}
        ),
    ),
    Control(
        name="constant-loop",
        file=NODES,
        anchor="        if integer_literal_is_nonzero(tree, text, token_spans, condition) {\n",
        mutant=(
            "        if false && integer_literal_is_nonzero(tree, text, token_spans, condition) {\n"
        ),
        kills=frozenset(
            {
                "constant_true_loop_loses_only_its_impossible_false_exit",
                "constant_true_empty_loop_keeps_its_cycle",
                "a_conditionless_for_is_not_a_constant_true_header",
            }
        ),
    ),
    Control(
        name="literal-if",
        file=NODES,
        anchor="        if is_bare_literal(tree, condition) {\n",
        mutant="        if false && is_bare_literal(tree, condition) {\n",
        kills=frozenset({"nested_literal_if_tests_cost_no_nodes_but_keep_their_forks"}),
    ),
    Control(
        name="ternary-loop",
        file=NODES,
        anchor="    let ternary_loop_collapsed = region.shape == Shape::Conditional\n",
        mutant="    let ternary_loop_collapsed = false && region.shape == Shape::Conditional\n",
        kills=frozenset(
            {
                "value_only_ternary_in_a_loop_condition_has_one_final_branch",
                "a_side_effecting_ternary_arm_keeps_the_expression_but_not_the_duplicate_header",
                "ternary_loop_cast_compare_and_load_variants_keep_their_expression_nodes",
            }
        ),
    ),
)


def _copy_crate(scratch: Path) -> None:
    # `shutil.copy`, not `copy2`, and then a touch: cargo decides freshness by
    # mtime, so a copy that keeps the source's old timestamps under a reused
    # target directory runs the *previous* run's last mutant as the baseline.
    # Observed on the first reuse of `--target-dir`: the baseline "failed" the
    # three ternary tests the ternary mutant kills.
    for name in ("Cargo.toml", "Cargo.lock"):
        shutil.copy(ROOT / name, scratch / name)
    shutil.copytree(
        ROOT / "crates",
        scratch / "crates",
        ignore=shutil.ignore_patterns("target"),
        copy_function=shutil.copy,
    )
    for path in scratch.rglob("*"):
        if path.is_file():
            path.touch()


def _run_parity_tests(scratch: Path, target_dir: Path) -> dict[str, str]:
    completed = subprocess.run(
        [
            "cargo",
            "test",
            "-p",
            "cindergraph",
            "--lib",
            "parity",
            "--",
            "--test-threads=4",
        ],
        cwd=scratch,
        env={**__import__("os").environ, "CARGO_TARGET_DIR": str(target_dir)},
        capture_output=True,
        text=True,
        check=False,
    )
    results = {
        match.group(1).rsplit("::", 1)[-1]: match.group(2)
        for match in TEST_LINE.finditer(completed.stdout)
    }
    if not results:
        raise RuntimeError(
            "no test lines parsed; cargo said:\n" + completed.stderr[-4000:]
        )
    return results


def run_control(control: Control, scratch: Path, target_dir: Path) -> list[str]:
    """Apply ``control`` in ``scratch`` and return the problems found."""
    path = scratch / control.file
    original = path.read_text()
    if original.count(control.anchor) != 1:
        return [f"{control.name}: anchor occurs {original.count(control.anchor)} times"]
    path.write_text(original.replace(control.anchor, control.mutant))
    try:
        results = _run_parity_tests(scratch, target_dir)
    finally:
        path.write_text(original)
    failed = {name for name, status in results.items() if status == "FAILED"}
    problems: list[str] = []
    for name in sorted(control.kills):
        if name not in results:
            problems.append(f"{control.name}: expected test {name} was not run")
    if failed != control.kills:
        problems.append(
            f"{control.name}: killed {sorted(failed)}, expected {sorted(control.kills)}"
        )
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--target-dir", type=Path, default=None)
    parser.add_argument("--only", action="append", default=None)
    args = parser.parse_args()
    controls = [
        control
        for control in CONTROLS
        if args.only is None or control.name in args.only
    ]
    if not controls:
        print("no control selected", file=sys.stderr)
        return 2
    with tempfile.TemporaryDirectory(prefix="cindergraph-mutants-") as tmp:
        scratch = Path(tmp)
        _copy_crate(scratch)
        target_dir = args.target_dir or scratch / "target"
        baseline = _run_parity_tests(scratch, target_dir)
        failing = sorted(
            name for name, status in baseline.items() if status == "FAILED"
        )
        if failing:
            print(f"baseline already fails {failing}; controls are meaningless")
            return 1
        print(f"baseline: {len(baseline)} parity tests, all passing")
        problems: list[str] = []
        for control in controls:
            found = run_control(control, scratch, target_dir)
            status = "ok" if not found else "PROBLEM"
            print(
                f"{control.name}: {status} (expects exactly {len(control.kills)} kills)"
            )
            problems.extend(found)
    for problem in problems:
        print(problem)
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
