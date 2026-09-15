"""Fail-closed tests for rerunning the crates.io publication job."""

from importlib.util import module_from_spec, spec_from_file_location
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[2]
SPEC = spec_from_file_location("publish_crate", ROOT / "tools/publish_crate.py")
assert SPEC is not None and SPEC.loader is not None
publish_crate = module_from_spec(SPEC)
SPEC.loader.exec_module(publish_crate)


def test_missing_version_is_published() -> None:
    assert publish_crate.publication_action("a" * 64, None) == "publish"


def test_identical_existing_version_is_safe_to_skip() -> None:
    assert publish_crate.publication_action("a" * 64, "A" * 64) == "skip"


def test_different_existing_version_fails_closed() -> None:
    with pytest.raises(RuntimeError, match="different checksum"):
        publish_crate.publication_action("a" * 64, "b" * 64)


def test_post_publish_checksum_waits_for_registry_visibility(monkeypatch) -> None:
    responses = iter((None, None, "a" * 64))
    sleeps: list[float] = []
    monkeypatch.setattr(
        publish_crate, "_remote_checksum", lambda _version: next(responses)
    )
    monkeypatch.setattr(publish_crate.time, "sleep", sleeps.append)
    publish_crate.require_matching_checksum(
        "a" * 64, "0.1.0", attempts=3, delay_seconds=0.25
    )
    assert sleeps == [0.25, 0.25]


def test_post_publish_checksum_rejects_different_registry_bytes(monkeypatch) -> None:
    monkeypatch.setattr(publish_crate, "_remote_checksum", lambda _version: "b" * 64)
    with pytest.raises(RuntimeError, match="different checksum"):
        publish_crate.require_matching_checksum("a" * 64, "0.1.0")


def test_post_publish_checksum_fails_when_version_never_appears(monkeypatch) -> None:
    monkeypatch.setattr(publish_crate, "_remote_checksum", lambda _version: None)
    with pytest.raises(RuntimeError, match="did not expose"):
        publish_crate.require_matching_checksum(
            "a" * 64, "0.1.0", attempts=2, delay_seconds=0
        )


def test_publisher_rejects_a_differently_named_reviewed_archive(
    monkeypatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(
        publish_crate, "_metadata", lambda: ("0.1.0", tmp_path / "target")
    )
    archive = tmp_path / "other-0.1.0.crate"
    archive.write_bytes(b"not consulted")
    with pytest.raises(RuntimeError, match="reviewed archive must be named"):
        publish_crate.main(archive)


def test_publisher_matches_reviewed_bytes_and_verifies_after_upload(
    monkeypatch, tmp_path: Path
) -> None:
    target = tmp_path / "target"
    packaged = target / "package" / "cindergraph-0.1.0.crate"
    packaged.parent.mkdir(parents=True)
    packaged.write_bytes(b"reviewed bytes")
    reviewed = tmp_path / "cindergraph-0.1.0.crate"
    reviewed.write_bytes(b"reviewed bytes")
    calls: list[list[str]] = []
    verified: list[tuple[str, str]] = []
    monkeypatch.setattr(publish_crate, "_metadata", lambda: ("0.1.0", target))
    monkeypatch.setattr(publish_crate, "_remote_checksum", lambda _version: None)
    monkeypatch.setattr(
        publish_crate.subprocess, "run", lambda args, **_kwargs: calls.append(args)
    )
    monkeypatch.setattr(
        publish_crate,
        "require_matching_checksum",
        lambda checksum, version: verified.append((checksum, version)),
    )

    assert publish_crate.main(reviewed) == 0
    assert calls == [
        ["cargo", "package", "-p", "cindergraph", "--locked"],
        ["cargo", "publish", "-p", "cindergraph", "--locked"],
    ]
    assert verified == [
        (publish_crate.hashlib.sha256(b"reviewed bytes").hexdigest(), "0.1.0")
    ]


def test_publisher_rejects_a_rebuild_that_differs_from_reviewed_bytes(
    monkeypatch, tmp_path: Path
) -> None:
    target = tmp_path / "target"
    packaged = target / "package" / "cindergraph-0.1.0.crate"
    packaged.parent.mkdir(parents=True)
    packaged.write_bytes(b"different rebuild")
    reviewed = tmp_path / "cindergraph-0.1.0.crate"
    reviewed.write_bytes(b"reviewed bytes")
    calls: list[list[str]] = []
    monkeypatch.setattr(publish_crate, "_metadata", lambda: ("0.1.0", target))
    monkeypatch.setattr(
        publish_crate.subprocess, "run", lambda args, **_kwargs: calls.append(args)
    )

    with pytest.raises(RuntimeError, match="publication rebuild differs"):
        publish_crate.main(reviewed)
    assert calls == [["cargo", "package", "-p", "cindergraph", "--locked"]]
