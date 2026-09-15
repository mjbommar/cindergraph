"""Archive-safety primitives used by the release artifact checker."""

from importlib.util import module_from_spec, spec_from_file_location
from email.parser import BytesParser
import json
from pathlib import Path
import zipfile

import pytest


ROOT = Path(__file__).resolve().parents[2]
SPEC = spec_from_file_location(
    "check_python_distribution", ROOT / "tools/check_python_distribution.py"
)
assert SPEC is not None and SPEC.loader is not None
checker = module_from_spec(SPEC)
SPEC.loader.exec_module(checker)

NORMALIZER_SPEC = spec_from_file_location(
    "normalize_wheel_sbom", ROOT / "tools/normalize_wheel_sbom.py"
)
assert NORMALIZER_SPEC is not None and NORMALIZER_SPEC.loader is not None
normalizer = module_from_spec(NORMALIZER_SPEC)
NORMALIZER_SPEC.loader.exec_module(normalizer)


def _valid_metadata():
    lines = [
        "Metadata-Version: 2.4",
        "Name: cindergraph",
        "Version: 0.1.0",
        "Summary: Tolerant, deterministic C source analysis backed by Rust",
        "License-Expression: Apache-2.0",
        "Requires-Python: >=3.12",
        "Description-Content-Type: text/markdown; charset=UTF-8; variant=GFM",
    ]
    lines.extend(f"Classifier: {value}" for value in checker.EXPECTED_CLASSIFIERS)
    lines.extend(("License-File: LICENSE", "License-File: NOTICE"))
    lines.extend(f"Project-URL: {value}" for value in checker.EXPECTED_PROJECT_URLS)
    lines.extend(
        (
            "Provides-Extra: graphs",
            "Requires-Dist: networkx>=3.0 ; extra == 'graphs'",
            "",
            "# Cindergraph",
        )
    )
    return BytesParser().parsebytes("\n".join(lines).encode())


def test_distribution_metadata_accepts_the_release_contract() -> None:
    assert checker.validate_distribution_metadata(_valid_metadata()) == "0.1.0"


@pytest.mark.parametrize(
    ("field", "value", "message"),
    [
        ("Name", "other", "name/version"),
        ("Requires-Python", ">=3.13", "Requires-Python"),
        ("Classifier", "Typing :: Stubs Only", "Classifier"),
        ("Project-URL", "Docs, https://example.invalid", "Project-URL"),
        ("Requires-Dist", "networkx>=4", "Requires-Dist"),
    ],
)
def test_distribution_metadata_rejects_registry_drift(
    field: str, value: str, message: str
) -> None:
    metadata = _valid_metadata()
    del metadata[field]
    metadata[field] = value
    with pytest.raises(ValueError, match=message):
        checker.validate_distribution_metadata(metadata)


def test_safe_archive_names_accepts_normal_posix_members() -> None:
    checker.safe_archive_names(["cindergraph-0.1.0/README.md", "pkg/module.py"])


@pytest.mark.parametrize("name", ["../escape", "/absolute", "a/../../b", "a\\b"])
def test_safe_archive_names_rejects_unsafe_members(name: str) -> None:
    with pytest.raises(ValueError, match="unsafe archive member"):
        checker.safe_archive_names([name])


def test_safe_archive_names_rejects_duplicates() -> None:
    with pytest.raises(ValueError, match="duplicate"):
        checker.safe_archive_names(["same", "same"])


def test_packaged_markdown_links_accept_relative_fragments_and_external_urls() -> None:
    documents = {
        "pkg/docs/index.md": (
            "[local](reference.md#result) [web](https://example.com/missing)"
        ),
        "pkg/docs/reference.md": "# Result",
    }
    checker.validate_packaged_markdown_links(documents, set(documents))


def test_packaged_markdown_links_accept_implicit_archive_directories() -> None:
    documents = {"pkg/docs/index.md": "[evidence](data/)"}
    members = {*documents, "pkg/docs/data/result.json"}
    checker.validate_packaged_markdown_links(documents, members)


def test_packaged_markdown_links_reject_checkout_only_targets() -> None:
    documents = {"pkg/docs/index.md": "[review](../../review/report.md)"}
    with pytest.raises(ValueError, match="packaged Markdown link is missing"):
        checker.validate_packaged_markdown_links(documents, set(documents))


def test_packaged_markdown_links_validate_same_page_and_duplicate_anchors() -> None:
    documents = {
        "pkg/index.md": (
            "# Start\n[second](#result-1)\n[other](reference.md#details)\n"
            "## Result\n## Result\n"
        ),
        "pkg/reference.md": "# Details",
    }
    checker.validate_packaged_markdown_links(documents, set(documents))


def test_packaged_markdown_links_reject_missing_anchor() -> None:
    documents = {
        "pkg/index.md": "[wrong](reference.md#missing)",
        "pkg/reference.md": "# Present",
    }
    with pytest.raises(ValueError, match="packaged Markdown anchor is missing"):
        checker.validate_packaged_markdown_links(documents, set(documents))


def test_sbom_reference_validation_accepts_a_closed_graph() -> None:
    checker.validate_sbom_references(
        {
            "metadata": {"component": {"bom-ref": "root"}},
            "components": [{"bom-ref": "dependency"}],
            "dependencies": [
                {"ref": "root", "dependsOn": ["dependency"]},
                {"ref": "dependency", "dependsOn": []},
            ],
        }
    )


def test_wheel_filename_tags_expand_compressed_platforms() -> None:
    tags = checker.wheel_filename_tags(
        "cindergraph-0.1.0-cp312-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        "0.1.0",
    )
    assert tags == {
        "cp312-abi3-manylinux_2_17_x86_64",
        "cp312-abi3-manylinux2014_x86_64",
    }


@pytest.mark.parametrize(
    "filename",
    [
        "other-0.1.0-cp312-abi3-win_amd64.whl",
        "cindergraph-9.9.9-cp312-abi3-win_amd64.whl",
        "cindergraph-0.1.0-cp312-abi3.whl",
    ],
)
def test_wheel_filename_tags_reject_wrong_identity_or_shape(filename: str) -> None:
    with pytest.raises(ValueError):
        checker.wheel_filename_tags(filename, "0.1.0")


@pytest.mark.parametrize(
    "sbom",
    [
        {
            "metadata": {"component": {"bom-ref": "same"}},
            "components": [{"bom-ref": "same"}],
            "dependencies": [],
        },
        {
            "metadata": {"component": {"bom-ref": "root"}},
            "components": [],
            "dependencies": [{"ref": "root", "dependsOn": ["missing"]}],
        },
        {
            "metadata": {"component": {"bom-ref": "root"}},
            "components": [],
            "dependencies": [{"ref": "missing", "dependsOn": []}],
        },
    ],
)
def test_sbom_reference_validation_rejects_ambiguous_or_open_graphs(
    sbom: dict[str, object],
) -> None:
    with pytest.raises(ValueError):
        checker.validate_sbom_references(sbom)


@pytest.mark.parametrize(
    ("original", "expected"),
    [
        (
            "path+file:///checkout/crates/cindergraph#0.1.0",
            "pkg:cargo/cindergraph@0.1.0",
        ),
        (
            "path+file:///D:/work/crates/cindergraph-python#0.1.0 bin-target-0",
            "pkg:cargo/cindergraph-python@0.1.0#bin-target-0",
        ),
        (
            "registry+https://github.com/rust-lang/crates.io-index#pyo3@0.29.2",
            "registry+https://github.com/rust-lang/crates.io-index#pyo3@0.29.2",
        ),
    ],
)
def test_sbom_package_ids_are_normalized_without_host_paths(
    original: str, expected: str
) -> None:
    assert normalizer._stable_bom_ref(original) == expected


def test_wheel_normalization_repairs_record_and_is_idempotent(tmp_path: Path) -> None:
    wheel = tmp_path / "sample-0.1.0-py3-none-any.whl"
    sbom_name = "sample-0.1.0.dist-info/sboms/cindergraph-python.cyclonedx.json"
    record_name = "sample-0.1.0.dist-info/RECORD"
    sbom = {
        "metadata": {
            "component": {
                "bom-ref": "path+file:///private/build/cindergraph-python#0.1.0"
            }
        },
        "dependencies": [
            {"ref": "path+file:///private/build/cindergraph-python#0.1.0"}
        ],
    }
    members = {
        sbom_name: json.dumps(sbom).encode(),
        record_name: f"{sbom_name},old,1\n{record_name},,\n".encode(),
    }
    with zipfile.ZipFile(wheel, "w") as archive:
        for name, body in members.items():
            archive.writestr(name, body)

    normalizer.normalize_wheel(wheel)
    first = wheel.read_bytes()
    normalizer.normalize_wheel(wheel)
    assert wheel.read_bytes() == first
    with zipfile.ZipFile(wheel) as archive:
        normalized = archive.read(sbom_name)
        assert b"path+file://" not in normalized
        assert b"pkg:cargo/cindergraph-python@0.1.0" in normalized
        rows = archive.read(record_name).decode().splitlines()
        assert rows[0].startswith(f"{sbom_name},sha256=")
        assert rows[1] == f"{record_name},,"
