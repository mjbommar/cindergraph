"""Fail-closed tests for post-publication PyPI reconciliation."""

from importlib.util import module_from_spec, spec_from_file_location
import hashlib
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[2]
SPEC = spec_from_file_location(
    "verify_pypi_release", ROOT / "tools/verify_pypi_release.py"
)
assert SPEC is not None and SPEC.loader is not None
verifier = module_from_spec(SPEC)
SPEC.loader.exec_module(verifier)


def test_local_artifacts_hashes_the_complete_upload_directory(tmp_path: Path) -> None:
    wheel = tmp_path / "cindergraph-0.1.0-cp312-abi3-win_amd64.whl"
    sdist = tmp_path / "cindergraph-0.1.0.tar.gz"
    wheel.write_bytes(b"wheel")
    sdist.write_bytes(b"sdist")
    assert verifier.local_artifacts(tmp_path) == {
        wheel.name: hashlib.sha256(b"wheel").hexdigest(),
        sdist.name: hashlib.sha256(b"sdist").hexdigest(),
    }


def test_local_artifacts_rejects_empty_or_unexpected_sets(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="empty"):
        verifier.local_artifacts(tmp_path)
    (tmp_path / "checksums.txt").write_text("not an upload", encoding="utf-8")
    with pytest.raises(ValueError, match="unexpected"):
        verifier.local_artifacts(tmp_path)


def test_identical_pypi_artifact_set_is_accepted_case_insensitively() -> None:
    verifier.require_identical_artifacts({"a.whl": "a" * 64}, {"a.whl": "A" * 64})


@pytest.mark.parametrize(
    ("published", "expected_plan"),
    [
        (None, "publish"),
        ({}, "publish"),
        ({"a.whl": "a" * 64}, "resume"),
        ({"a.whl": "a" * 64, "b.tar.gz": "b" * 64}, "skip"),
    ],
)
def test_pypi_publication_plan_handles_fresh_partial_and_complete_releases(
    published: dict[str, str] | None, expected_plan: str
) -> None:
    expected = {"a.whl": "a" * 64, "b.tar.gz": "b" * 64}
    assert verifier.publication_plan(expected, published) == expected_plan


@pytest.mark.parametrize(
    "published",
    [
        {"foreign.whl": "a" * 64},
        {"a.whl": "b" * 64},
    ],
)
def test_pypi_publication_plan_rejects_foreign_existing_state(
    published: dict[str, str],
) -> None:
    with pytest.raises(RuntimeError, match="not a reviewed subset"):
        verifier.publication_plan({"a.whl": "a" * 64}, published)


@pytest.mark.parametrize("kind", ["missing", "unexpected", "checksum"])
def test_pypi_artifact_drift_is_rejected(kind: str) -> None:
    expected = {"a.whl": "a" * 64}
    published = {"a.whl": "a" * 64}
    if kind == "missing":
        published.clear()
    elif kind == "unexpected":
        published["extra.tar.gz"] = "b" * 64
    else:
        published["a.whl"] = "b" * 64
    with pytest.raises(RuntimeError, match="differs from reviewed upload"):
        verifier.require_identical_artifacts(expected, published)


def test_pypi_verifier_waits_for_release_visibility(monkeypatch) -> None:
    expected = {"a.whl": "a" * 64}
    responses = iter((None, None, expected))
    sleeps: list[float] = []
    monkeypatch.setattr(verifier, "remote_artifacts", lambda _version: next(responses))
    monkeypatch.setattr(verifier.time, "sleep", sleeps.append)
    verifier.wait_for_release(expected, "0.1.0", attempts=3, delay_seconds=0.25)
    assert sleeps == [0.25, 0.25]


def test_pypi_verifier_fails_when_release_never_appears(monkeypatch) -> None:
    monkeypatch.setattr(verifier, "remote_artifacts", lambda _version: None)
    with pytest.raises(RuntimeError, match="did not expose"):
        verifier.wait_for_release(
            {"a.whl": "a" * 64}, "0.1.0", attempts=2, delay_seconds=0
        )
