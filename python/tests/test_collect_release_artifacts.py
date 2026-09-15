"""Fail closed while gathering cross-job artifacts for PyPI."""

from importlib.util import module_from_spec, spec_from_file_location
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[2]
SPEC = spec_from_file_location(
    "collect_release_artifacts", ROOT / "tools/collect_release_artifacts.py"
)
assert SPEC is not None and SPEC.loader is not None
collector = module_from_spec(SPEC)
SPEC.loader.exec_module(collector)


def artifact_tree(root: Path, version: str = "0.1.0") -> None:
    """Create one minimal, uniquely named file for each producer."""
    platforms = {
        "wheels-ubuntu-24.04-x86_64": "manylinux_2_17_x86_64.manylinux2014_x86_64",
        "wheels-ubuntu-24.04-arm-aarch64": "manylinux_2_17_aarch64.manylinux2014_aarch64",
        "wheels-macos-15-intel-x86_64": "macosx_10_12_x86_64",
        "wheels-macos-15-aarch64": "macosx_11_0_arm64",
        "wheels-windows-latest-x64": "win_amd64",
    }
    for directory, suffix in collector.EXPECTED.items():
        parent = root / directory
        parent.mkdir(parents=True)
        if suffix == ".whl":
            name = f"cindergraph-{version}-cp312-abi3-{platforms[directory]}.whl"
        else:
            name = f"cindergraph-{version}.tar.gz"
        (parent / name).write_bytes(directory.encode())


def test_collect_requires_and_copies_the_complete_unique_set(tmp_path: Path) -> None:
    source = tmp_path / "downloaded"
    destination = tmp_path / "dist"
    artifact_tree(source)
    copied = collector.collect(source, destination, "0.1.0")
    assert len(copied) == 6
    assert {path.name for path in copied} == {
        path.name for path in destination.iterdir()
    }


def test_collect_rejects_a_missing_producer(tmp_path: Path) -> None:
    source = tmp_path / "downloaded"
    artifact_tree(source)
    missing = source / next(iter(collector.EXPECTED))
    for path in missing.iterdir():
        path.unlink()
    missing.rmdir()
    with pytest.raises(ValueError, match="missing="):
        collector.collect(source, tmp_path / "dist", "0.1.0")


def test_collect_rejects_duplicate_wheel_filenames(tmp_path: Path) -> None:
    source = tmp_path / "downloaded"
    artifact_tree(source)
    wheel_dirs = [
        source / name for name, suffix in collector.EXPECTED.items() if suffix == ".whl"
    ]
    first = next(wheel_dirs[0].iterdir()).name
    second = next(wheel_dirs[1].iterdir())
    second.rename(second.with_name(first))
    with pytest.raises(ValueError, match="duplicate filenames"):
        collector.collect(source, tmp_path / "dist", "0.1.0")


def test_collect_rejects_a_wheel_from_the_wrong_platform_producer(
    tmp_path: Path,
) -> None:
    source = tmp_path / "downloaded"
    artifact_tree(source)
    linux = source / "wheels-ubuntu-24.04-x86_64"
    wheel = next(linux.iterdir())
    wheel.rename(wheel.with_name(wheel.name.replace("x86_64", "ppc64le")))
    with pytest.raises(ValueError, match="platform does not match producer"):
        collector.collect(source, tmp_path / "dist", "0.1.0")


@pytest.mark.parametrize("kind", ["version", "type", "extra", "nonempty"])
def test_collect_rejects_malformed_sets(tmp_path: Path, kind: str) -> None:
    source = tmp_path / "downloaded"
    destination = tmp_path / "dist"
    artifact_tree(source)
    if kind == "version":
        wheel = next((source / "wheels-ubuntu-24.04-x86_64").iterdir())
        wheel.rename(wheel.with_name(wheel.name.replace("0.1.0", "9.9.9")))
    elif kind == "type":
        wheel = next((source / "wheels-ubuntu-24.04-x86_64").iterdir())
        wheel.rename(wheel.with_suffix(".zip"))
    elif kind == "extra":
        (source / "unexpected").mkdir()
    else:
        destination.mkdir()
        (destination / "old.whl").write_bytes(b"old")
    with pytest.raises(ValueError):
        collector.collect(source, destination, "0.1.0")
