#!/usr/bin/env python3
"""Publish the core crate, or verify an identical prior publication.

This makes a release-workflow rerun safe after crates.io succeeded but a later
job failed. An existing version is accepted only when its registry checksum
matches the freshly packaged local crate byte-for-byte.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time
from typing import Literal
from urllib.error import HTTPError
from urllib.request import Request, urlopen


CRATE = "cindergraph"
REGISTRY_API = "https://crates.io/api/v1/crates"


def publication_action(
    local_checksum: str, remote_checksum: str | None
) -> Literal["publish", "skip"]:
    """Return the safe action, rejecting a same-version checksum mismatch."""
    if remote_checksum is None:
        return "publish"
    if local_checksum.lower() == remote_checksum.lower():
        return "skip"
    raise RuntimeError(
        "crates.io already has this version with a different checksum: "
        f"local={local_checksum}, remote={remote_checksum}"
    )


def _metadata() -> tuple[str, Path]:
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(result.stdout)
    package = next(item for item in metadata["packages"] if item["name"] == CRATE)
    return package["version"], Path(metadata["target_directory"])


def _remote_checksum(version: str) -> str | None:
    request = Request(
        f"{REGISTRY_API}/{CRATE}/{version}",
        headers={"User-Agent": "cindergraph-release-workflow"},
    )
    try:
        with urlopen(request, timeout=30) as response:  # noqa: S310 - fixed HTTPS host
            payload = json.load(response)
    except HTTPError as error:
        if error.code == 404:
            return None
        raise
    checksum = payload["version"]["checksum"]
    if not isinstance(checksum, str) or len(checksum) != 64:
        raise RuntimeError("crates.io returned an invalid version checksum")
    return checksum


def require_matching_checksum(
    expected: str,
    version: str,
    *,
    attempts: int = 12,
    delay_seconds: float = 5.0,
) -> None:
    """Wait for crates.io visibility and require the uploaded bytes to match."""
    for attempt in range(attempts):
        remote = _remote_checksum(version)
        if remote is not None:
            publication_action(expected, remote)
            return
        if attempt + 1 < attempts:
            time.sleep(delay_seconds)
    raise RuntimeError(f"crates.io did not expose {CRATE} {version} after publication")


def main(reviewed_archive: Path) -> int:
    version, target = _metadata()
    expected_name = f"{CRATE}-{version}.crate"
    if reviewed_archive.name != expected_name:
        raise RuntimeError(
            f"reviewed archive must be named {expected_name}, got {reviewed_archive.name}"
        )
    reviewed_checksum = hashlib.sha256(reviewed_archive.read_bytes()).hexdigest()
    subprocess.run(["cargo", "package", "-p", CRATE, "--locked"], check=True)
    archive = target / "package" / f"{CRATE}-{version}.crate"
    local_checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    if local_checksum != reviewed_checksum:
        raise RuntimeError(
            "publication rebuild differs from reviewed crate: "
            f"reviewed={reviewed_checksum}, rebuilt={local_checksum}"
        )
    remote_checksum = _remote_checksum(version)

    if publication_action(reviewed_checksum, remote_checksum) == "skip":
        print(f"{CRATE} {version} is already published with the identical checksum")
        return 0

    subprocess.run(["cargo", "publish", "-p", CRATE, "--locked"], check=True)
    require_matching_checksum(reviewed_checksum, version)
    return 0


if __name__ == "__main__":
    try:
        if len(sys.argv) != 2:
            raise RuntimeError("usage: publish_crate.py REVIEWED.crate")
        raise SystemExit(main(Path(sys.argv[1])))
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"release error: {error}", file=sys.stderr)
        raise SystemExit(1) from error
