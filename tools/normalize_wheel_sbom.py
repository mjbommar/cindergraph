#!/usr/bin/env python3
"""Remove build-host paths from Maturin's Rust SBOM and repair wheel RECORD."""

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import tempfile
from typing import Any
from urllib.parse import unquote
import zipfile


def _stable_bom_ref(value: str) -> str:
    """Turn cargo-cyclonedx's absolute path package IDs into stable IDs."""
    if not value.startswith("path+file://"):
        return value
    location, separator, identity = value.partition("#")
    if not separator or not identity:
        raise ValueError(f"unrecognised path package ID: {value!r}")
    version, space, target = identity.partition(" ")
    crate = PurePosixPath(unquote(location.removeprefix("path+file://"))).name
    if not crate or not version:
        raise ValueError(f"unrecognised path package ID: {value!r}")
    stable = f"pkg:cargo/{crate}@{version}"
    return f"{stable}#{target}" if space else stable


def _normalise(value: Any) -> Any:
    if isinstance(value, str):
        return _stable_bom_ref(value)
    if isinstance(value, list):
        return [_normalise(item) for item in value]
    if isinstance(value, dict):
        return {key: _normalise(item) for key, item in value.items()}
    return value


def _digest(body: bytes) -> str:
    encoded = base64.urlsafe_b64encode(hashlib.sha256(body).digest()).rstrip(b"=")
    return "sha256=" + encoded.decode("ascii")


def normalize_wheel(path: Path) -> None:
    """Rewrite one wheel atomically while retaining member order and metadata."""
    if path.suffix != ".whl":
        raise ValueError(f"not a wheel: {path}")
    with zipfile.ZipFile(path) as source:
        infos = source.infolist()
        names = [info.filename for info in infos]
        if len(names) != len(set(names)):
            raise ValueError("wheel contains duplicate members")
        sboms = [
            name
            for name in names
            if name.endswith(".dist-info/sboms/cindergraph-python.cyclonedx.json")
        ]
        records = [name for name in names if name.endswith(".dist-info/RECORD")]
        if len(sboms) != 1 or len(records) != 1:
            raise ValueError(
                "wheel must contain exactly one Cindergraph SBOM and RECORD"
            )
        sbom_name, record_name = sboms[0], records[0]
        bodies = {info.filename: source.read(info.filename) for info in infos}

    sbom = _normalise(json.loads(bodies[sbom_name]))
    bodies[sbom_name] = (json.dumps(sbom, indent=2, ensure_ascii=False) + "\n").encode()
    if b"path+file://" in bodies[sbom_name]:
        raise ValueError("SBOM still contains an absolute path package ID")

    rows = list(csv.reader(io.StringIO(bodies[record_name].decode())))
    recorded = [row[0] for row in rows]
    if len(recorded) != len(set(recorded)) or set(recorded) != set(names):
        raise ValueError("RECORD member set does not match wheel")
    output = io.StringIO(newline="")
    writer = csv.writer(output, lineterminator="\n")
    for member in names:
        if member == record_name:
            writer.writerow((member, "", ""))
        else:
            writer.writerow((member, _digest(bodies[member]), len(bodies[member])))
    bodies[record_name] = output.getvalue().encode()

    descriptor, temporary = tempfile.mkstemp(prefix=path.name + ".", dir=path.parent)
    os.close(descriptor)
    temporary_path = Path(temporary)
    try:
        with zipfile.ZipFile(temporary_path, "w") as target:
            for info in infos:
                target.writestr(
                    info,
                    bodies[info.filename],
                    compress_type=info.compress_type,
                    compresslevel=9,
                )
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("wheels", nargs="+", type=Path)
    args = parser.parse_args()
    for wheel in args.wheels:
        normalize_wheel(wheel)
        print(f"normalised {wheel}")


if __name__ == "__main__":
    main()
