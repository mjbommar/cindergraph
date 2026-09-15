#!/usr/bin/env python3
"""Require PyPI to expose exactly the locally reviewed release artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import time
import tomllib
from urllib.error import HTTPError
from urllib.request import Request, urlopen


PROJECT = "cindergraph"
PYPI_API = "https://pypi.org/pypi"


def local_artifacts(directory: Path) -> dict[str, str]:
    """Hash the complete wheel/sdist upload set."""
    paths = sorted(path for path in directory.iterdir() if path.is_file())
    if not paths:
        raise ValueError("local PyPI artifact set is empty")
    unexpected = [
        path.name
        for path in paths
        if not (path.name.endswith(".whl") or path.name.endswith(".tar.gz"))
    ]
    if unexpected:
        raise ValueError(f"unexpected local PyPI artifacts: {unexpected}")
    return {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}


def remote_artifacts(version: str) -> dict[str, str] | None:
    """Read filename-to-SHA256 metadata from PyPI's fixed project API."""
    request = Request(
        f"{PYPI_API}/{PROJECT}/{version}/json",
        headers={"User-Agent": "cindergraph-release-workflow"},
    )
    try:
        with urlopen(request, timeout=30) as response:  # noqa: S310 - fixed HTTPS host
            payload = json.load(response)
    except HTTPError as error:
        if error.code == 404:
            return None
        raise
    urls = payload.get("urls")
    if not isinstance(urls, list):
        raise RuntimeError("PyPI returned no release artifact list")
    result: dict[str, str] = {}
    for item in urls:
        if not isinstance(item, dict):
            raise RuntimeError("PyPI returned a malformed release artifact")
        filename = item.get("filename")
        digests = item.get("digests")
        checksum = digests.get("sha256") if isinstance(digests, dict) else None
        if (
            not isinstance(filename, str)
            or not isinstance(checksum, str)
            or len(checksum) != 64
            or filename in result
        ):
            raise RuntimeError("PyPI returned malformed or duplicate artifact metadata")
        result[filename] = checksum
    return result


def require_identical_artifacts(
    expected: dict[str, str], published: dict[str, str]
) -> None:
    """Reject missing, additional, or byte-different published artifacts."""
    missing = set(expected).difference(published)
    unexpected = set(published).difference(expected)
    mismatched = {
        name
        for name in set(expected).intersection(published)
        if expected[name].lower() != published[name].lower()
    }
    if missing or unexpected or mismatched:
        raise RuntimeError(
            "PyPI artifact set differs from reviewed upload: "
            f"missing={sorted(missing)}, unexpected={sorted(unexpected)}, "
            f"checksum_mismatch={sorted(mismatched)}"
        )


def publication_plan(expected: dict[str, str], published: dict[str, str] | None) -> str:
    """Choose a safe fresh, partial-rerun, or completed-release action."""
    if not published:
        return "publish"
    unexpected = set(published).difference(expected)
    mismatched = {
        name
        for name in set(expected).intersection(published)
        if expected[name].lower() != published[name].lower()
    }
    if unexpected or mismatched:
        raise RuntimeError(
            "existing PyPI release is not a reviewed subset: "
            f"unexpected={sorted(unexpected)}, "
            f"checksum_mismatch={sorted(mismatched)}"
        )
    if set(published) == set(expected):
        return "skip"
    return "resume"


def wait_for_release(
    expected: dict[str, str],
    version: str,
    *,
    attempts: int = 12,
    delay_seconds: float = 5.0,
) -> None:
    """Wait for PyPI visibility, then reconcile the complete release."""
    for attempt in range(attempts):
        published = remote_artifacts(version)
        if published is not None:
            require_identical_artifacts(expected, published)
            return
        if attempt + 1 < attempts:
            time.sleep(delay_seconds)
    raise RuntimeError(f"PyPI did not expose {PROJECT} {version} after publication")


def main(directory: Path, *, preflight: bool = False) -> None:
    """Verify one local upload directory against the workspace release version."""
    root = Path(__file__).resolve().parents[1]
    workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    version = workspace["workspace"]["package"]["version"]
    expected = local_artifacts(directory)
    if preflight:
        print(publication_plan(expected, remote_artifacts(version)))
        return
    wait_for_release(expected, version)
    print(f"PyPI {PROJECT} {version}: {len(expected)} reviewed artifacts verified")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--preflight", action="store_true")
    args = parser.parse_args()
    main(args.directory, preflight=args.preflight)
