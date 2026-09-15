#!/usr/bin/env python3
"""Validate the contents and registry metadata of a packaged Rust crate."""

import argparse
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tomllib


EXPECTED_PACKAGE = {
    "name": "cindergraph",
    "edition": "2021",
    "rust-version": "1.88",
    "license": "Apache-2.0",
    "repository": "https://github.com/mjbommar/cindergraph",
    "documentation": "https://docs.rs/cindergraph",
    "readme": "README.md",
    "publish": ["crates-io"],
}
EXPECTED_DEPENDENCIES = {"regex": "1.10"}
EXPECTED_DEV_DEPENDENCIES = {"serde_json": "1.0"}
MAX_ARCHIVE_BYTES = 2 * 1024 * 1024
GIT_SHA1 = re.compile(r"[0-9a-f]{40}")


def validate_archive_size(size: int) -> None:
    """Keep the compressed crate beneath Cindergraph's reviewed size budget."""
    if size <= 0:
        raise ValueError("crate archive is empty")
    if size > MAX_ARCHIVE_BYTES:
        raise ValueError(
            f"crate archive exceeds {MAX_ARCHIVE_BYTES}-byte project budget: {size}"
        )


def validate_manifest(document: dict[str, object]) -> str:
    """Require the normalized registry manifest to match release policy."""
    package = document.get("package")
    if not isinstance(package, dict):
        raise ValueError("packaged Cargo.toml has no package table")
    for field, expected in EXPECTED_PACKAGE.items():
        if package.get(field) != expected:
            raise ValueError(f"packaged Cargo.toml package.{field} changed")
    version = package.get("version")
    if not isinstance(version, str) or not version:
        raise ValueError("packaged Cargo.toml has no package version")

    library = document.get("lib")
    if library != {"name": "cindergraph", "path": "src/lib.rs"}:
        raise ValueError("packaged Cargo.toml library target changed")
    tests = document.get("test")
    if tests != [{"name": "readme", "path": "tests/readme.rs"}]:
        raise ValueError("packaged Cargo.toml README test target changed")

    dependencies = document.get("dependencies")
    if not isinstance(dependencies, dict) or set(dependencies) != set(
        EXPECTED_DEPENDENCIES
    ):
        raise ValueError("packaged Cargo.toml dependency set changed")
    for name, expected_version in EXPECTED_DEPENDENCIES.items():
        dependency = dependencies[name]
        if dependency != {"version": expected_version}:
            raise ValueError(f"packaged dependency {name} is not registry-only")
    dev_dependencies = document.get("dev-dependencies")
    if not isinstance(dev_dependencies, dict) or set(dev_dependencies) != set(
        EXPECTED_DEV_DEPENDENCIES
    ):
        raise ValueError("packaged Cargo.toml dev-dependency set changed")
    for name, expected_version in EXPECTED_DEV_DEPENDENCIES.items():
        dependency = dev_dependencies[name]
        if dependency != {"version": expected_version}:
            raise ValueError(f"packaged dev-dependency {name} is not registry-only")
    return version


def validate_vcs_info(document: dict[str, object], expected_sha: str) -> None:
    """Require Cargo provenance for the exact clean release commit."""
    git = document.get("git")
    if not isinstance(git, dict):
        raise ValueError("packaged crate has no Git provenance")
    sha = git.get("sha1")
    if not isinstance(sha, str) or GIT_SHA1.fullmatch(sha) is None:
        raise ValueError("packaged crate has an invalid Git SHA-1")
    if sha != expected_sha:
        raise ValueError(
            f"packaged crate Git SHA-1 {sha} does not equal HEAD {expected_sha}"
        )
    if git.get("dirty", False) is not False:
        raise ValueError("packaged crate was built from a dirty Git tree")
    if document.get("path_in_vcs") != "crates/cindergraph":
        raise ValueError("packaged crate has an unexpected path_in_vcs")


def _safe_member_names(names: list[str]) -> None:
    if len(names) != len(set(names)):
        raise ValueError("crate archive contains duplicate member names")
    for name in names:
        path = PurePosixPath(name)
        if not name or "\\" in name or path.is_absolute() or ".." in path.parts:
            raise ValueError(f"unsafe crate archive member: {name!r}")


def check(archive: Path, repository: Path) -> None:
    """Validate one Cargo archive without extracting it."""
    validate_archive_size(archive.stat().st_size)
    with tarfile.open(archive, "r:gz") as package:
        members = package.getmembers()
        names = [member.name for member in members]
        _safe_member_names(names)
        if any(not (member.isfile() or member.isdir()) for member in members):
            raise ValueError("crate archive contains links or special files")
        if any(member.mode & 0o022 for member in members):
            raise ValueError("crate archive contains group/world-writable members")
        roots = {PurePosixPath(name).parts[0] for name in names}
        if len(roots) != 1:
            raise ValueError(f"crate archive has unexpected roots: {sorted(roots)}")
        root = next(iter(roots))

        required = {
            f"{root}/.cargo_vcs_info.json",
            f"{root}/Cargo.lock",
            f"{root}/Cargo.toml",
            f"{root}/Cargo.toml.orig",
            f"{root}/LICENSE",
            f"{root}/NOTICE",
            f"{root}/README.md",
            f"{root}/src/lib.rs",
            f"{root}/tests/readme.rs",
        }
        missing = required.difference(names)
        if missing:
            raise ValueError(f"crate archive is missing: {sorted(missing)}")

        def read(name: str) -> bytes:
            matches = [member for member in members if member.name == name]
            if len(matches) != 1 or not matches[0].isfile():
                raise ValueError(f"expected one regular crate member: {name}")
            stream = package.extractfile(matches[0])
            if stream is None:
                raise ValueError(f"could not read crate member: {name}")
            return stream.read()

        for name in ("LICENSE", "NOTICE", "README.md"):
            if read(f"{root}/{name}") != (repository / name).read_bytes():
                raise ValueError(f"{archive}: {name} differs from repository copy")

        manifest = tomllib.loads(read(f"{root}/Cargo.toml").decode("utf-8"))
        version = validate_manifest(manifest)
        if root != f"cindergraph-{version}":
            raise ValueError("crate archive root and manifest version disagree")
        expected_sha = subprocess.run(
            ["git", "-C", str(repository), "rev-parse", "HEAD"],
            check=True,
            stdout=subprocess.PIPE,
            text=True,
        ).stdout.strip()
        validate_vcs_info(
            json.loads(read(f"{root}/.cargo_vcs_info.json")), expected_sha
        )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    args = parser.parse_args()
    check(args.archive, Path(__file__).resolve().parents[1])
    print(f"{args.archive}: Cargo metadata, contents and notices verified")
