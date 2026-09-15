#!/usr/bin/env python3
"""Fail-closed structural checks for Cindergraph wheels and source archives."""

from __future__ import annotations

import argparse
import base64
import csv
from email.message import Message
from email.parser import BytesParser
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import posixpath
import re
import stat
import tarfile
import zipfile


EXPECTED_CLASSIFIERS = [
    "Development Status :: 2 - Pre-Alpha",
    "Intended Audience :: Developers",
    "Intended Audience :: Science/Research",
    "Programming Language :: Python :: 3",
    "Programming Language :: Python :: 3.12",
    "Programming Language :: Python :: 3.13",
    "Programming Language :: Python :: 3.14",
    "Programming Language :: Python :: Implementation :: CPython",
    "Programming Language :: Rust",
    "Topic :: Software Development :: Libraries :: Python Modules",
    "Typing :: Typed",
]
EXPECTED_PROJECT_URLS = [
    "Changelog, https://github.com/mjbommar/cindergraph/blob/main/CHANGELOG.md",
    ("Documentation, https://github.com/mjbommar/cindergraph/blob/main/docs/README.md"),
    "Issues, https://github.com/mjbommar/cindergraph/issues",
    "Repository, https://github.com/mjbommar/cindergraph",
]


def safe_archive_names(names: list[str]) -> None:
    """Reject duplicate, absolute, platform-dependent, or traversing names."""
    if len(names) != len(set(names)):
        raise ValueError("archive contains duplicate member names")
    for name in names:
        path = PurePosixPath(name)
        if not name or "\\" in name or path.is_absolute() or ".." in path.parts:
            raise ValueError(f"unsafe archive member: {name!r}")


def validate_distribution_metadata(metadata: Message) -> str:
    """Validate the registry-facing metadata shared by wheels and sdists."""
    version = metadata["Version"]
    if metadata["Name"] != "cindergraph" or not version:
        raise ValueError(
            "distribution name/version metadata does not identify Cindergraph"
        )
    expected_scalars = {
        "Metadata-Version": "2.4",
        "Summary": "Tolerant, deterministic C source analysis backed by Rust",
        "License-Expression": "Apache-2.0",
        "Requires-Python": ">=3.12",
        "Description-Content-Type": "text/markdown; charset=UTF-8; variant=GFM",
    }
    for field, expected in expected_scalars.items():
        if metadata[field] != expected:
            raise ValueError(f"distribution {field} changed")
    expected_lists = {
        "Classifier": EXPECTED_CLASSIFIERS,
        "License-File": ["LICENSE", "NOTICE"],
        "Project-URL": EXPECTED_PROJECT_URLS,
        "Provides-Extra": ["graphs"],
        "Requires-Dist": ["networkx>=3.0 ; extra == 'graphs'"],
    }
    for field, expected in expected_lists.items():
        if metadata.get_all(field, []) != expected:
            raise ValueError(f"distribution {field} metadata changed")
    return version


MARKDOWN_LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
MARKDOWN_HEADING = re.compile(r"^ {0,3}#{1,6}\s+(.+?)\s*#*\s*$")


def markdown_anchors(body: str) -> set[str]:
    """Return GitHub-style anchors for ATX headings outside fenced blocks."""
    anchors: set[str] = set()
    occurrences: dict[str, int] = {}
    fenced = False
    for line in body.splitlines():
        if line.lstrip().startswith(("```", "~~~")):
            fenced = not fenced
            continue
        if fenced:
            continue
        match = MARKDOWN_HEADING.fullmatch(line)
        if match is None:
            continue
        heading = re.sub(r"<[^>]*>", "", match.group(1)).lower()
        heading = re.sub(r"[^\w\- ]", "", heading)
        base = re.sub(r"\s", "-", heading)
        suffix = occurrences.get(base, 0)
        occurrences[base] = suffix + 1
        anchors.add(base if suffix == 0 else f"{base}-{suffix}")
    return anchors


def validate_packaged_markdown_links(
    documents: dict[str, str], members: set[str]
) -> None:
    """Require relative Markdown links to resolve inside the distribution."""
    for document, body in documents.items():
        for match in MARKDOWN_LINK.finditer(body):
            target = match.group(1).strip()
            if target.startswith("<") and target.endswith(">"):
                target = target[1:-1]
            if (
                not target
                or target.startswith("/")
                or "://" in target
                or target.startswith("mailto:")
            ):
                continue
            location, _, fragment = target.partition("#")
            path = location.split("?", 1)[0]
            resolved = posixpath.normpath(
                posixpath.join(
                    posixpath.dirname(document), path or posixpath.basename(document)
                )
            )
            directory_prefix = resolved.rstrip("/") + "/"
            if resolved not in members and not any(
                member.startswith(directory_prefix) for member in members
            ):
                raise ValueError(
                    f"packaged Markdown link is missing: "
                    f"{document} -> {target} ({resolved})"
                )
            if fragment and resolved in documents:
                anchors = markdown_anchors(documents[resolved])
                if fragment not in anchors:
                    raise ValueError(
                        f"packaged Markdown anchor is missing: {document} -> {target}"
                    )


def validate_sbom_references(sbom: dict[str, object]) -> None:
    """Require every CycloneDX dependency reference to name one component."""
    references: list[str] = []

    def collect(value: object) -> None:
        if not isinstance(value, dict):
            return
        reference = value.get("bom-ref")
        if isinstance(reference, str):
            references.append(reference)
        children = value.get("components", [])
        if isinstance(children, list):
            for child in children:
                collect(child)

    metadata = sbom.get("metadata", {})
    if isinstance(metadata, dict):
        collect(metadata.get("component"))
    components = sbom.get("components", [])
    if isinstance(components, list):
        for component in components:
            collect(component)
    if len(references) != len(set(references)):
        raise ValueError("wheel SBOM contains duplicate component references")

    known = set(references)
    dependencies = sbom.get("dependencies", [])
    if not isinstance(dependencies, list):
        raise ValueError("wheel SBOM dependencies are not a list")
    rows: list[str] = []
    linked: list[str] = []
    for dependency in dependencies:
        if not isinstance(dependency, dict) or not isinstance(
            dependency.get("ref"), str
        ):
            raise ValueError("wheel SBOM contains a malformed dependency row")
        rows.append(dependency["ref"])
        depends_on = dependency.get("dependsOn", [])
        if not isinstance(depends_on, list) or not all(
            isinstance(reference, str) for reference in depends_on
        ):
            raise ValueError("wheel SBOM contains malformed dependency targets")
        linked.extend(depends_on)
    if len(rows) != len(set(rows)):
        raise ValueError("wheel SBOM contains duplicate dependency rows")
    unresolved = (set(rows) | set(linked)) - known
    if unresolved:
        raise ValueError(
            f"wheel SBOM contains unresolved references: {sorted(unresolved)}"
        )


def wheel_filename_tags(filename: str, version: str) -> set[str]:
    """Expand the compressed compatibility tags in a Cindergraph wheel name."""
    prefix = f"cindergraph-{version}-"
    if not filename.startswith(prefix) or not filename.endswith(".whl"):
        raise ValueError(
            f"wheel filename does not identify version {version}: {filename}"
        )
    fields = filename[len(prefix) : -len(".whl")].split("-")
    if len(fields) != 3:
        raise ValueError(f"wheel filename has an unexpected tag shape: {filename}")
    python_tags, abi_tags, platform_tags = (field.split(".") for field in fields)
    return {
        f"{python_tag}-{abi_tag}-{platform_tag}"
        for python_tag in python_tags
        for abi_tag in abi_tags
        for platform_tag in platform_tags
    }


def _wheel(path: Path) -> None:
    with zipfile.ZipFile(path) as archive:
        infos = archive.infolist()
        names = [item.filename for item in infos]
        safe_archive_names(names)
        for item in infos:
            kind = (item.external_attr >> 16) & 0o170000
            if kind not in (0, stat.S_IFREG, stat.S_IFDIR):
                raise ValueError(f"non-regular wheel member: {item.filename}")

        def exactly_one(suffix: str) -> str:
            matches = [name for name in names if name.endswith(suffix)]
            if len(matches) != 1:
                raise ValueError(f"expected one {suffix!r}, found {len(matches)}")
            return matches[0]

        metadata_name = exactly_one(".dist-info/METADATA")
        record_name = exactly_one(".dist-info/RECORD")
        wheel_name = exactly_one(".dist-info/WHEEL")
        native_matches = [
            name
            for name in names
            if name.startswith("cindergraph/_native") and name.endswith((".so", ".pyd"))
        ]
        if len(native_matches) != 1:
            raise ValueError(
                f"expected one native extension, found {len(native_matches)}"
            )
        native_name = native_matches[0]
        sbom_name = exactly_one(".dist-info/sboms/cindergraph-python.cyclonedx.json")
        for required in (
            "cindergraph/py.typed",
            "cindergraph/_native/__init__.pyi",
        ):
            if required not in names:
                raise ValueError(f"wheel is missing {required}")
        exactly_one(".dist-info/licenses/LICENSE")
        exactly_one(".dist-info/licenses/NOTICE")
        if archive.getinfo(native_name).file_size == 0:
            raise ValueError("native extension is empty")

        metadata = BytesParser().parsebytes(archive.read(metadata_name))
        version = validate_distribution_metadata(metadata)
        if not metadata_name.startswith(f"cindergraph-{version}.dist-info/"):
            raise ValueError("wheel metadata directory and version disagree")
        wheel_metadata = archive.read(wheel_name).decode("utf-8")
        if (
            "Root-Is-Purelib: false" not in wheel_metadata
            or "cp312-abi3" not in wheel_metadata
        ):
            raise ValueError("wheel is not the expected CPython 3.12+ ABI3 binary")
        wheel_headers = BytesParser().parsebytes(archive.read(wheel_name))
        embedded_tags = set(wheel_headers.get_all("Tag", []))
        filename_tags = wheel_filename_tags(path.name, version)
        if not embedded_tags or embedded_tags != filename_tags:
            raise ValueError(
                f"wheel filename and embedded tags disagree: "
                f"{sorted(filename_tags)} != {sorted(embedded_tags)}"
            )

        rows = list(csv.reader(io.StringIO(archive.read(record_name).decode("utf-8"))))
        if {row[0] for row in rows} != set(names):
            raise ValueError("RECORD member set does not match the wheel")
        for member, digest, size in rows:
            if member == record_name:
                if digest or size:
                    raise ValueError("RECORD must not hash itself")
                continue
            body = archive.read(member)
            expected = (
                base64.urlsafe_b64encode(hashlib.sha256(body).digest())
                .rstrip(b"=")
                .decode()
            )
            if digest != f"sha256={expected}" or size != str(len(body)):
                raise ValueError(f"bad RECORD entry for {member}")

        sbom = json.loads(archive.read(sbom_name))
        if "path+file://" in json.dumps(sbom):
            raise ValueError("wheel SBOM leaks a build-host path package ID")
        validate_sbom_references(sbom)
        component = sbom.get("metadata", {}).get("component", {})
        if (
            sbom.get("bomFormat") != "CycloneDX"
            or component.get("name") != "cindergraph-python"
            or component.get("version") != version
        ):
            raise ValueError("wheel SBOM does not identify this binding release")
        components = {
            (item.get("name"), item.get("version"))
            for item in sbom.get("components", [])
        }
        if ("cindergraph", version) not in components or not any(
            name == "pyo3" for name, _ in components
        ):
            raise ValueError("wheel SBOM omits required Rust components")


def _sdist(path: Path) -> None:
    with tarfile.open(path, "r:gz") as archive:
        members = archive.getmembers()
        names = [item.name for item in members]
        safe_archive_names(names)
        if any(not (item.isfile() or item.isdir()) for item in members):
            raise ValueError("sdist contains links or special files")
        if any(item.mode & 0o022 for item in members):
            raise ValueError("sdist contains group/world-writable members")
        roots = {PurePosixPath(name).parts[0] for name in names}
        if len(roots) != 1:
            raise ValueError(f"unexpected sdist root: {sorted(roots)}")
        package_root = next(iter(roots))
        if not package_root.startswith("cindergraph-"):
            raise ValueError(f"unexpected sdist root: {package_root}")
        root = package_root + "/"
        required = {
            root + "PKG-INFO",
            root + "Cargo.lock",
            root + "EXTRACTION.md",
            root + "LICENSE",
            root + "NOTICE",
            root + "README.md",
            root + "pyproject.toml",
            root + "python/cindergraph/py.typed",
            root + "python/cindergraph/_native/__init__.pyi",
            root + "crates/cindergraph/src/lib.rs",
            root + "crates/cindergraph-python/src/lib.rs",
            root + "docs/architecture/glaurung.md",
            root + "docs/releasing.md",
        }
        missing = required.difference(names)
        if missing:
            raise ValueError(f"sdist is missing: {sorted(missing)}")
        pkg_info = archive.extractfile(root + "PKG-INFO")
        if pkg_info is None:
            raise ValueError("sdist PKG-INFO is not a regular file")
        metadata = BytesParser().parsebytes(pkg_info.read())
        version = validate_distribution_metadata(metadata)
        if package_root != f"cindergraph-{version}":
            raise ValueError("sdist root and metadata version disagree")
        documents = {
            item.name: archive.extractfile(item).read().decode("utf-8")
            for item in members
            if item.isfile() and item.name.endswith(".md")
        }
        validate_packaged_markdown_links(documents, set(names))
        forbidden = (root + ".git/", root + "target/", root + "review/")
        leaked = [name for name in names if name.startswith(forbidden)]
        if leaked:
            raise ValueError(f"sdist contains checkout-only paths: {leaked[:3]}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("artifacts", nargs="+", type=Path)
    args = parser.parse_args()
    for artifact in args.artifacts:
        if artifact.suffix == ".whl":
            _wheel(artifact)
        elif artifact.name.endswith(".tar.gz"):
            _sdist(artifact)
        else:
            raise ValueError(f"unsupported distribution: {artifact}")
        print(f"checked {artifact}")


if __name__ == "__main__":
    main()
