"""Fail closed unless a release tag, package version and dated heading agree."""

from __future__ import annotations

import argparse
from datetime import date
from pathlib import Path
import re
import tomllib


ROOT = Path(__file__).resolve().parents[1]


def validate_release(
    tag: str, version: str, changelog: str, *, today: date | None = None
) -> date:
    """Return the release date or raise ``ValueError`` for inconsistent input."""
    expected_tag = f"v{version}"
    if tag != expected_tag:
        raise ValueError(f"tag {tag!r} does not equal {expected_tag!r}")

    prefix = f"## {version} — "
    headings = [line for line in changelog.splitlines() if line.startswith(prefix)]
    if len(headings) != 1:
        raise ValueError(
            f"expected exactly one changelog heading beginning {prefix!r}, "
            f"found {len(headings)}"
        )
    heading = headings[0]
    match = re.fullmatch(
        rf"## {re.escape(version)} — (\d{{4}}-\d{{2}}-\d{{2}})", heading
    )
    if match is None:
        raise ValueError(f"release heading must end in an ISO date: {heading!r}")
    try:
        released = date.fromisoformat(match.group(1))
    except ValueError as error:
        raise ValueError(f"release heading has an invalid date: {heading!r}") from error
    if released > (today or date.today()):
        raise ValueError(f"release heading has a future date: {heading!r}")
    return released


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("tag", help="exact Git tag, for example v0.1.0")
    args = parser.parse_args()
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    version = manifest["workspace"]["package"]["version"]
    changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    try:
        released = validate_release(args.tag, version, changelog)
    except ValueError as error:
        parser.error(str(error))
    print(f"release metadata agrees: {args.tag} ({released.isoformat()})")


if __name__ == "__main__":
    main()
