"""Keep registry-facing metadata internally consistent."""

from datetime import date
from pathlib import Path
import importlib.util
import re
import tomllib
from typing import Any

import pytest


ROOT = Path(__file__).resolve().parents[2]


def _release_checker():
    path = ROOT / "tools" / "check_release_version.py"
    spec = importlib.util.spec_from_file_location("check_release_version", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _toml(path: Path) -> dict[str, Any]:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def test_python_metadata_uses_current_license_and_project_urls() -> None:
    project = _toml(ROOT / "pyproject.toml")["project"]
    assert isinstance(project, dict)
    assert project["license"] == "Apache-2.0"
    assert project["license-files"] == ["LICENSE", "NOTICE"]
    assert project["requires-python"] == ">=3.12"
    classifiers = project["classifiers"]
    assert isinstance(classifiers, list)
    assert not any(str(value).startswith("License ::") for value in classifiers)
    for version in ("3.12", "3.13", "3.14"):
        assert f"Programming Language :: Python :: {version}" in classifiers
    assert "Programming Language :: Python :: Implementation :: CPython" in classifiers
    assert "Typing :: Typed" in classifiers
    urls = project["urls"]
    assert isinstance(urls, dict)
    assert set(urls) == {"Changelog", "Documentation", "Issues", "Repository"}
    assert all(str(url).startswith("https://") for url in urls.values())
    extras = project["optional-dependencies"]
    assert set(extras) == {"graphs"}
    assert extras["graphs"] == ["networkx>=3.0"]


def test_workspace_version_matches_changelog_and_publish_policy() -> None:
    workspace = _toml(ROOT / "Cargo.toml")
    package = workspace["workspace"]["package"]
    assert isinstance(package, dict)
    version = package["version"]
    assert isinstance(version, str)
    changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    assert re.search(rf"^## {re.escape(version)}(?:\s|$)", changelog, re.MULTILINE)

    core = _toml(ROOT / "crates" / "cindergraph" / "Cargo.toml")["package"]
    binding = _toml(ROOT / "crates" / "cindergraph-python" / "Cargo.toml")["package"]
    assert isinstance(core, dict)
    assert isinstance(binding, dict)
    assert core["publish"] == ["crates-io"]
    assert binding["publish"] is False

    maturin = _toml(ROOT / "pyproject.toml")["tool"]["maturin"]
    assert maturin["strip"] is True


def test_registry_readme_uses_portable_links() -> None:
    """Relative links in embedded metadata would point into registry pages."""
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    for target in re.findall(r"(?<!!)\[[^]]+\]\(([^)]+)\)", readme):
        assert "://" in target or target.startswith("#"), target


def test_release_workflow_has_fail_closed_partial_publish_recovery() -> None:
    workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    script = (ROOT / "tools/publish_crate.py").read_text(encoding="utf-8")
    assert "python3 tools/publish_crate.py" in workflow
    assert "CARGO_REGISTRY_TOKEN" in workflow
    assert "different checksum" in script
    assert 'subprocess.run(["cargo", "publish"' in script


def test_release_preserves_reviewed_crate_through_publication() -> None:
    workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    assert "name: crate-package" in workflow
    assert "path: target/package/cindergraph-*.crate" in workflow
    publish = workflow.split("\n  publish-crate:\n", 1)[1].split(
        "\n  publish-pypi:\n", 1
    )[0]
    assert "name: crate-package" in publish
    assert "path: reviewed-crate" in publish
    assert 'test "${#archives[@]}" -eq 1' in publish
    assert 'tools/check_crate_notices.py "${archives[0]}"' in publish
    assert 'tools/publish_crate.py "${archives[0]}"' in publish


def test_pypi_publish_revalidates_a_non_overlapping_artifact_set() -> None:
    workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    job = workflow.split("\n  publish-pypi:\n", 1)[1]
    assert "merge-multiple: true" not in job
    assert 'pattern: "*"' not in job
    assert "pattern: wheels-*" in job
    assert "name: sdist" in job
    assert "path: downloaded/sdist" in job
    assert "name: crate-package" not in job
    assert "python3 tools/collect_release_artifacts.py downloaded dist" in job
    assert "python3 tools/check_python_distribution.py dist/*" in job
    assert "uvx --from twine==7.0.0 twine check --strict dist/*" in job
    assert "packages-dir: dist" in job
    assert "attestations: true" in job
    assert "tools/verify_pypi_release.py --preflight dist" in job
    assert "id: pypi-preflight" in job
    assert "if: steps.pypi-preflight.outputs.action != 'skip'" in job
    assert (
        "skip-existing: ${{ steps.pypi-preflight.outputs.action == 'resume' }}" in job
    )
    assert "python3 tools/verify_pypi_release.py dist" in job
    assert "pypi-attestations==0.0.30" in job
    assert "pypi-attestations verify pypi" in job
    assert "--repository https://github.com/mjbommar/cindergraph" in job
    assert 'test "${#artifacts[@]}" -eq 6' in job
    assert '"pypi:${artifact##*/}"' in job
    assert "contents: read" in job
    assert "id-token: write" in job


def test_ci_and_release_smoke_the_installed_graphs_extra() -> None:
    ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    for workflow in (ci, release):
        assert "smoke_graphs_installed.py" in workflow
        assert "[graphs]" in workflow


def test_ci_and_release_type_check_an_installed_wheel_consumer() -> None:
    ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    assert "tests/typing/consumer.py" in ci
    assert "uv run ty check" in ci
    assert "tests/typing/consumer.py" in release
    assert "uvx --from ty==0.0.80 ty check" in release
    for workflow in (ci, release):
        assert "--error-on-warning" in workflow
        assert "tests/typing/graphs_consumer.py" in workflow
        assert '"$typing_dir/graphs_consumer.py"' in workflow


def test_manual_release_dispatch_can_never_publish() -> None:
    workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    publish_jobs = workflow.split("\n  publish-crate:\n", 1)[1]
    condition = (
        "if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')"
    )
    assert publish_jobs.count(condition) == 2


def test_release_tag_requires_an_exact_dated_changelog_heading() -> None:
    checker = _release_checker()
    today = date(2026, 9, 14)
    assert checker.validate_release(
        "v1.2.3", "1.2.3", "## 1.2.3 — 2026-09-14", today=today
    ) == date(2026, 9, 14)
    rejected = [
        ("v1.2.4", "1.2.3", "## 1.2.3 — 2026-09-14"),
        ("v1.2.3", "1.2.3", "## 1.2.3 — unreleased"),
        ("v1.2.3", "1.2.3", "## 1.2.3 — 2026-02-30"),
        ("v1.2.3", "1.2.3", "## 1.2.30 — 2026-09-14"),
        ("v1.2.3", "1.2.3", "## 1.2.3 — 2026-09-14\n## 1.2.3 — 2026-09-15"),
        ("v1.2.3", "1.2.3", "## 1.2.3 — 2026-09-15"),
    ]
    for tag, version, changelog in rejected:
        with pytest.raises(ValueError):
            checker.validate_release(tag, version, changelog, today=today)


def test_release_workflow_invokes_exact_version_checker() -> None:
    workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    assert 'python3 tools/check_release_version.py "$GITHUB_REF_NAME"' in workflow
    assert 'grep -F "## $version"' not in workflow


def test_release_wheels_pin_tooling_and_compatibility_policy() -> None:
    workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    assert "PyO3/maturin-action@e83996d129638aa358a18fbd1dfb82f0b0fb5d3b" in workflow
    assert "maturin-version: v1.15.0" in workflow
    assert workflow.count('manylinux: "2_17"') == 2
    assert "--compatibility pypi" in workflow
    assert "manylinux: auto" not in workflow
    assert workflow.count("tools/check_python_distribution.py") == 3
    assert workflow.count("docker-options: -e SOURCE_DATE_EPOCH") == 2
    assert workflow.count("tools/normalize_wheel_sbom.py") == 1
    assert "dist/*.whl dist-rebuilt/*.whl" in workflow
    assert "Require byte-reproducible wheel packaging" in workflow
    assert "dist-rebuilt" in workflow
    assert workflow.count("uvx --from twine==7.0.0 twine check --strict") == 3


def test_release_sdist_is_rebuilt_and_compared_before_upload() -> None:
    workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    sdist_job = workflow.split("\n  sdist:\n", 1)[1].split("\n  publish-crate:\n", 1)[0]
    assert sdist_job.count("command: sdist") == 2
    assert "Require byte-reproducible sdist packaging" in sdist_job
    assert 'Path("dist").glob("*.tar.gz")' in sdist_job
    assert 'Path("dist-rebuilt").glob("*.tar.gz")' in sdist_job
    assert "if not first or first != second" in sdist_job
    assert "for version in 3.12 3.13 3.14" in sdist_job
    assert 'uv venv --python "$version"' in sdist_job


def test_ci_smokes_sdist_on_every_supported_cpython() -> None:
    workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    sdist_step = workflow.split("- name: Build and install the source distribution", 1)[
        1
    ]
    assert "for version in 3.12 3.13 3.14" in sdist_step
    assert 'uv venv --python "$version"' in sdist_step


def test_ci_and_release_smoke_the_graphs_extra_from_sdist() -> None:
    ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    ci_sdist = ci.split("- name: Build and install the source distribution", 1)[1]
    release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    release_sdist = release.split("\n  sdist:\n", 1)[1].split(
        "\n  publish-crate:\n", 1
    )[0]
    for job in (ci_sdist, release_sdist):
        assert "sdist-graphs" in job
        assert "${archives[0]}[graphs]" in job
        assert "smoke_graphs_installed.py" in job


def test_ci_and_release_enforce_rust_advisory_audits() -> None:
    install = "cargo install cargo-audit --version 0.22.2 --locked"
    audit = "cargo audit"
    deny_action = (
        "EmbarkStudios/cargo-deny-action@9c67a826e39827395bfa619cd85cb2aaeb43cfaa"
    )
    ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    assert install in ci
    assert install in release
    assert audit in ci
    assert audit in release
    assert deny_action in ci
    assert deny_action in release
    assert (ROOT / "deny.toml").is_file()


def test_ci_checks_registry_metadata_for_both_python_artifact_types() -> None:
    workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    command = "uvx --from twine==7.0.0 twine check --strict"
    assert workflow.count(command) == 2
    assert f"{command} target/ci-wheels/*.whl" in workflow
    assert f"{command} target/ci-sdist/*.tar.gz" in workflow


def test_ci_and_release_require_reproducible_crate_archives() -> None:
    for path in (
        ROOT / ".github/workflows/ci.yml",
        ROOT / ".github/workflows/release.yml",
    ):
        workflow = path.read_text(encoding="utf-8")
        assert "Require byte-reproducible crate packaging" in workflow
        assert 'first=$(sha256sum "$archive"' in workflow
        assert 'second=$(sha256sum "$archive"' in workflow
        assert 'test "$first" = "$second"' in workflow


def test_ci_and_release_validate_and_test_the_packaged_crate() -> None:
    for path in (
        ROOT / ".github/workflows/ci.yml",
        ROOT / ".github/workflows/release.yml",
    ):
        workflow = path.read_text(encoding="utf-8")
        assert 'python3 tools/check_crate_notices.py "$archive"' in workflow
        assert "target/package/cindergraph-$version/Cargo.toml" in workflow
        assert "--locked --all-features" in workflow


def test_workflows_do_not_persist_checkout_credentials() -> None:
    for path in (
        ROOT / ".github/workflows/ci.yml",
        ROOT / ".github/workflows/release.yml",
    ):
        workflow = path.read_text(encoding="utf-8")
        checkouts = workflow.count("uses: actions/checkout@")
        assert checkouts > 0
        assert workflow.count("persist-credentials: false") == checkouts


def test_release_builds_do_not_restore_uv_caches() -> None:
    workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    setups = workflow.count("uses: astral-sh/setup-uv@")
    assert setups > 0
    assert workflow.count("enable-cache: false") == setups


def test_release_guide_treats_crates_token_as_bootstrap_only() -> None:
    guide = (ROOT / "docs/releasing.md").read_text(encoding="utf-8")
    assert "Cargo currently requires token-based authentication" not in guide
    assert "bootstrap crates.io release" in guide
    assert "rust-lang/crates-io-auth-action" in guide
    assert "revoke the bootstrap token" in guide


def test_ci_and_release_run_the_pinned_workflow_security_audit() -> None:
    command = "uvx --from zizmor==1.30.1 zizmor .github/workflows"
    for path in (
        ROOT / ".github/workflows/ci.yml",
        ROOT / ".github/workflows/release.yml",
    ):
        workflow = path.read_text(encoding="utf-8")
        assert workflow.count(command) == 1


def test_ci_and_release_validate_python_metadata_and_classifiers() -> None:
    schema = (
        "uvx --from validate-pyproject==0.26 --with packaging==26.0 "
        "validate-pyproject pyproject.toml"
    )
    classifiers = (
        "uvx --from trove-classifiers==2026.6.1.19 "
        "python tools/check_trove_classifiers.py"
    )
    for path in (
        ROOT / ".github/workflows/ci.yml",
        ROOT / ".github/workflows/release.yml",
    ):
        workflow = path.read_text(encoding="utf-8")
        assert workflow.count(schema) == 1
        assert workflow.count(classifiers) == 1


def test_ci_and_release_audit_all_published_python_extras() -> None:
    export = "uv export --locked --no-dev --all-extras --no-emit-project"
    audit = "uvx --from pip-audit==2.10.1 pip-audit --strict"
    for path in (
        ROOT / ".github/workflows/ci.yml",
        ROOT / ".github/workflows/release.yml",
    ):
        workflow = path.read_text(encoding="utf-8")
        assert workflow.count(export) == 1
        assert workflow.count(audit) == 1
        assert "--require-hashes -r /dev/stdin" in workflow
        assert "set -o pipefail" in workflow


def test_workflows_define_safe_concurrency_policies() -> None:
    ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    assert "group: ci-${{ github.workflow }}-${{ github.ref }}" in ci
    assert "cancel-in-progress: true" in ci

    release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    assert "group: cindergraph-release" in release
    assert "cancel-in-progress: false" in release


def test_every_workflow_job_has_a_display_name() -> None:
    expected = {
        ROOT / ".github/workflows/ci.yml": 2,
        ROOT / ".github/workflows/release.yml": 5,
    }
    for path, job_count in expected.items():
        workflow = path.read_text(encoding="utf-8")
        jobs = workflow.split("\njobs:\n", 1)[1]
        assert len(re.findall(r"^  [a-z][a-z-]+:\n", jobs, re.MULTILINE)) == job_count
        assert len(re.findall(r"^    name: .+$", jobs, re.MULTILINE)) == job_count


def test_ci_normalizes_wheel_sboms_before_distribution_checks() -> None:
    workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    normalize = workflow.index("tools/normalize_wheel_sbom.py")
    validate = workflow.index("tools/check_python_distribution.py")
    assert normalize < validate


def test_every_workflow_action_is_pinned_to_a_full_commit() -> None:
    action = re.compile(r"^\s*- uses:\s+([^\s#]+)", re.MULTILINE)
    full_commit = re.compile(r"^[^@]+@[0-9a-f]{40}$")
    for path in sorted((ROOT / ".github" / "workflows").glob("*.yml")):
        references = action.findall(path.read_text(encoding="utf-8"))
        assert references, path
        assert all(full_commit.fullmatch(reference) for reference in references), (
            path,
            references,
        )
