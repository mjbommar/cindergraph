"""Unit tests for the packaged Rust crate contract."""

from importlib.util import module_from_spec, spec_from_file_location
from pathlib import Path
from typing import Any

import pytest


ROOT = Path(__file__).resolve().parents[2]
SPEC = spec_from_file_location(
    "check_crate_notices", ROOT / "tools/check_crate_notices.py"
)
assert SPEC is not None and SPEC.loader is not None
checker = module_from_spec(SPEC)
SPEC.loader.exec_module(checker)


def _manifest() -> dict[str, Any]:
    return {
        "package": {**checker.EXPECTED_PACKAGE, "version": "0.1.0"},
        "lib": {"name": "cindergraph", "path": "src/lib.rs"},
        "test": [{"name": "readme", "path": "tests/readme.rs"}],
        "dependencies": {
            name: {"version": version}
            for name, version in checker.EXPECTED_DEPENDENCIES.items()
        },
        "dev-dependencies": {
            name: {"version": version}
            for name, version in checker.EXPECTED_DEV_DEPENDENCIES.items()
        },
    }


def test_packaged_manifest_accepts_the_release_contract() -> None:
    assert checker.validate_manifest(_manifest()) == "0.1.0"


def test_packaged_vcs_info_accepts_the_exact_clean_commit() -> None:
    sha = "0123456789abcdef0123456789abcdef01234567"
    checker.validate_vcs_info(
        {"git": {"sha1": sha}, "path_in_vcs": "crates/cindergraph"}, sha
    )
    checker.validate_vcs_info(
        {"git": {"sha1": sha, "dirty": False}, "path_in_vcs": "crates/cindergraph"},
        sha,
    )


@pytest.mark.parametrize(
    ("document", "expected_sha", "message"),
    [
        ({}, "0" * 40, "no Git provenance"),
        (
            {"git": {"sha1": "short"}, "path_in_vcs": "crates/cindergraph"},
            "0" * 40,
            "invalid Git SHA-1",
        ),
        (
            {"git": {"sha1": "1" * 40}, "path_in_vcs": "crates/cindergraph"},
            "0" * 40,
            "does not equal HEAD",
        ),
        (
            {
                "git": {"sha1": "0" * 40, "dirty": True},
                "path_in_vcs": "crates/cindergraph",
            },
            "0" * 40,
            "dirty Git tree",
        ),
        (
            {"git": {"sha1": "0" * 40}, "path_in_vcs": "crates/other"},
            "0" * 40,
            "path_in_vcs",
        ),
    ],
)
def test_packaged_vcs_info_rejects_unattributable_archives(
    document: dict[str, object], expected_sha: str, message: str
) -> None:
    with pytest.raises(ValueError, match=message):
        checker.validate_vcs_info(document, expected_sha)


def test_crate_archive_size_budget_accepts_a_nonempty_bounded_package() -> None:
    checker.validate_archive_size(1)
    checker.validate_archive_size(checker.MAX_ARCHIVE_BYTES)


@pytest.mark.parametrize("size", [0, -1, 2 * 1024 * 1024 + 1])
def test_crate_archive_size_budget_rejects_empty_or_oversized_packages(
    size: int,
) -> None:
    with pytest.raises(ValueError, match="empty|exceeds"):
        checker.validate_archive_size(size)


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("name", "other"),
        ("edition", "2024"),
        ("rust-version", "1.89"),
        ("license", "MIT"),
        ("repository", "https://example.invalid"),
        ("publish", False),
    ],
)
def test_packaged_manifest_rejects_registry_metadata_drift(
    field: str, value: object
) -> None:
    manifest = _manifest()
    manifest["package"][field] = value
    with pytest.raises(ValueError, match=field):
        checker.validate_manifest(manifest)


def test_packaged_manifest_rejects_path_dependency() -> None:
    manifest = _manifest()
    manifest["dependencies"]["regex"]["path"] = "../regex"
    with pytest.raises(ValueError, match="registry-only"):
        checker.validate_manifest(manifest)


def test_packaged_manifest_rejects_path_dev_dependency() -> None:
    manifest = _manifest()
    manifest["dev-dependencies"]["serde_json"]["path"] = "../serde_json"
    with pytest.raises(ValueError, match="registry-only"):
        checker.validate_manifest(manifest)


def test_packaged_manifest_rejects_missing_readme_test() -> None:
    manifest = _manifest()
    manifest["test"] = []
    with pytest.raises(ValueError, match="README test target"):
        checker.validate_manifest(manifest)


@pytest.mark.parametrize("name", ["../escape", "/absolute", "a/../../b", "a\\b"])
def test_crate_archive_names_reject_unsafe_paths(name: str) -> None:
    with pytest.raises(ValueError, match="unsafe crate archive member"):
        checker._safe_member_names([name])


def test_crate_archive_names_reject_duplicates() -> None:
    with pytest.raises(ValueError, match="duplicate"):
        checker._safe_member_names(["same", "same"])
