# QA iteration 15: release runners and pre-upload smoke gates

The release workflow still selected `macos-13`, which GitHub has retired.
Verified against the official [retirement notice](https://github.blog/changelog/2025-09-19-github-actions-macos-13-runner-image-is-closing-down/)
and current [runner image inventory](https://github.com/actions/runner-images).
Selected `macos-15-intel` for x86-64 and `macos-15` for ARM. Linux now uses
`ubuntu-24.04` and `ubuntu-24.04-arm`, respectively, so both wheel architectures
can be tested natively. Windows remains on its existing x64 runner.

Each release wheel job now installs every produced wheel without dependencies
into an explicit new virtual environment and runs the installed smoke checker
before artifact upload. The shell handles Windows's Scripts/python.exe path.
An empty wheel glob is an error, and both wheel/sdist uploads fail if files are
missing. Publication triggers, protected environment and credentials were not
changed; nothing was triggered or published.

An initial local trial using `uv run --isolated --with` instead of an explicit
venv failed the smoke check's strict sys.prefix ownership condition because uv
uses a layered cache environment. The workflow uses the already tested explicit
venv installation route instead; the ownership guard was not weakened.

Local validation:

- `actionlint .github/workflows/ci.yml .github/workflows/release.yml` found only
  the two new labels absent from this machine's older built-in runner list.
- `actionlint -ignore 'label "(ubuntu-24.04-arm|macos-15-intel)" is unknown'
  .github/workflows/ci.yml .github/workflows/release.yml` passed. Those two labels
  were checked against GitHub's current inventory above, not treated as custom
  self-hosted labels or added to a permanent lint suppression.
- The retained iteration-12 Linux wheel again passed its explicit CPython 3.12
  environment's smoke checker. This verifies the install/check mechanism, not
  newly built macOS, ARM or Windows artifacts.
- `git diff --check` passed.

No local Rust/Python analysis code changed; their full suites were not rerun.
Remote execution of the revised matrix is still required. Baseline is `edf2777`
plus pending QA work; all changes remain local and uncommitted.
