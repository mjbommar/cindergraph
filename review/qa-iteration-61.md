# QA iteration 61: exact release identity gate

The tag validation in `release.yml` compared `GITHUB_REF_NAME` with the Cargo
version but only grepped the changelog for the substring `## $version`. The
current `## 0.1.0 — unreleased` heading therefore satisfied a release-tag run,
despite the operator checklist explicitly requiring a dated heading.

`tools/check_release_version.py` now performs one testable, fail-closed check:

- the tag must equal `v` plus the workspace package version exactly;
- exactly one changelog heading may begin with that version;
- the whole heading must be `## X.Y.Z — YYYY-MM-DD`;
- the date must exist and must not be in the future.

Historical dates remain valid so an exact release commit can be rerun. The
workflow calls the checker directly, and the previous permissive grep is gone.
Regression cases cover a mismatched tag, unreleased marker, impossible date,
prefix-only version match, duplicate headings and future date. Running the
checker against the current tree correctly exits nonzero because 0.1.0 is
still unreleased.

## Registry-name research

Read-only official API requests on 2026-09-14 returned HTTP 404 for both
`https://crates.io/api/v1/crates/cindergraph` and
`https://pypi.org/pypi/cindergraph/json`. The crates.io response explicitly
said that crate `cindergraph` does not exist; PyPI returned `Not Found`.
This is evidence that neither public project existed at that instant, not a
reservation, ownership proof or guarantee that publication will succeed.
Both must be checked again immediately before the human-approved first release.

## Validation and artifact boundary

- Ten focused release-metadata tests passed, including the new validator.
- The full Python/design suite passed 3,032 tests, skipped ten and deselected
  three optional Joern cases.
- Ruff, ty, generated stubs, Rust formatting, actionlint 1.7.12 and
  `git diff --check` passed.
- The complete Rust and artifact gates remain QA 60 evidence because no Rust or
  shipped package content changed in this iteration.

The exact QA 60 wheel and sdist were inspected: `.github`, `python/tests` and
`tools/check_release_version.py` are absent from both archives. Their hashes
therefore still identify the current shipped contents, while they do not test
the new checkout-only release guard itself.

No registry mutation, ownership request, workflow dispatch, commit, push or tag
was performed.
