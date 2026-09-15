"""The standalone Cargo archive must retain the auxiliary source fixtures."""

from pathlib import Path


def test_auxiliary_corpus_is_identical_in_rust_package() -> None:
    root = Path(__file__).resolve().parents[2]
    original = root / "tests/decbench_corpus/src"
    bundled = root / "crates/cindergraph/tests/decbench_corpus/src"
    sources = sorted(original.glob("*.c"))
    assert len(sources) == 14
    assert {p.name for p in sources} == {p.name for p in bundled.glob("*.c")}
    for path in sources:
        assert path.read_bytes() == (bundled / path.name).read_bytes(), path.name
