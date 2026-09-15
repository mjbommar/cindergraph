#!/usr/bin/env python3
"""Collect a complete, non-overlapping release artifact set for PyPI."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import shutil
import tomllib


EXPECTED = {
    "wheels-ubuntu-24.04-x86_64": ".whl",
    "wheels-ubuntu-24.04-arm-aarch64": ".whl",
    "wheels-macos-15-intel-x86_64": ".whl",
    "wheels-macos-15-aarch64": ".whl",
    "wheels-windows-latest-x64": ".whl",
    "sdist": ".tar.gz",
}

EXPECTED_WHEEL_PLATFORMS = {
    "wheels-ubuntu-24.04-x86_64": (
        r"(?:manylinux_2_17_x86_64(?:\.manylinux2014_x86_64)?|manylinux2014_x86_64)"
    ),
    "wheels-ubuntu-24.04-arm-aarch64": (
        r"(?:manylinux_2_17_aarch64(?:\.manylinux2014_aarch64)?|manylinux2014_aarch64)"
    ),
    "wheels-macos-15-intel-x86_64": r"macosx_\d+_\d+_x86_64",
    "wheels-macos-15-aarch64": r"macosx_\d+_\d+_arm64",
    "wheels-windows-latest-x64": r"win_amd64",
}


def collect(source: Path, destination: Path, version: str) -> list[Path]:
    """Validate and copy one artifact from every required producer."""
    entries = {path.name: path for path in source.iterdir()}
    unexpected = set(entries).difference(EXPECTED)
    missing = set(EXPECTED).difference(entries)
    if unexpected or missing:
        raise ValueError(
            f"release artifact directories disagree: "
            f"missing={sorted(missing)}, unexpected={sorted(unexpected)}"
        )

    selected: list[tuple[str, Path]] = []
    for directory, suffix in EXPECTED.items():
        root = entries[directory]
        if not root.is_dir() or root.is_symlink():
            raise ValueError(f"release artifact is not a real directory: {root}")
        if any(path.is_symlink() for path in root.rglob("*")):
            raise ValueError(f"release artifact directory contains a symlink: {root}")
        files = [path for path in root.rglob("*") if path.is_file()]
        if len(files) != 1:
            raise ValueError(f"expected one file in {root}, found {len(files)}")
        artifact = files[0]
        if not artifact.name.endswith(suffix):
            raise ValueError(f"unexpected artifact type: {artifact.name}")
        selected.append((directory, artifact))

    names = [path.name for _, path in selected]
    if len(names) != len(set(names)):
        raise ValueError(f"release artifacts have duplicate filenames: {names}")

    for directory, artifact in selected:
        suffix = EXPECTED[directory]
        if suffix == ".whl":
            prefix = f"cindergraph-{version}-cp312-abi3-"
            if not artifact.name.startswith(prefix):
                raise ValueError(
                    f"wheel identity does not match cindergraph {version} ABI3: "
                    f"{artifact.name}"
                )
            platform = artifact.name.removeprefix(prefix).removesuffix(".whl")
            expected_platform = EXPECTED_WHEEL_PLATFORMS[directory]
            if re.fullmatch(expected_platform, platform) is None:
                raise ValueError(
                    f"wheel platform does not match producer {directory}: "
                    f"{artifact.name}"
                )
        elif artifact.name != f"cindergraph-{version}.tar.gz":
            raise ValueError(f"sdist version does not match {version}: {artifact.name}")
    destination.mkdir(parents=True, exist_ok=True)
    if any(destination.iterdir()):
        raise ValueError(f"release destination is not empty: {destination}")
    copied = []
    for _, artifact in selected:
        target = destination / artifact.name
        shutil.copy2(artifact, target)
        copied.append(target)
    return copied


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    version = manifest["workspace"]["package"]["version"]
    for artifact in collect(args.source, args.destination, version):
        print(artifact)


if __name__ == "__main__":
    main()
