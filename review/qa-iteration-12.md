# QA iteration 12: isolated wheel installation across CPython

Built a fresh release ABI3 wheel from `edf2777` plus pending iterations 6–11:

`TMPDIR=/home/mjbommar/.cache/cindergraph uv run maturin build --release --out target/qa-wheel`

Artifact: `cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl`.
SHA-256: `f26a52f66adecf0e929613863daa236e1f3304df0f08ebca06e54e1abe1ab872`.

Installed that same artifact with `uv pip install --no-deps --python ...`
into three new environments under
`/home/mjbommar/.cache/cindergraph/wheel-qa.mLX8n9/`. Ran each environment's
`bin/python -I /home/mjbommar/projects/personal/cindergraph/tools/smoke_installed.py`
with the working directory outside the repository. All passed:

- CPython 3.12.13;
- CPython 3.13.12;
- CPython 3.14.3.

The smoke checker asserts that Python and native modules load from the virtual
environment, not the checkout. It checks packaged stubs, `py.typed`, LICENSE
and NOTICE, scalar/local-pointer analysis, summary recovery uncertainty and
JSON PDG export. Only Cindergraph was installed: no NetworkX or other runtime
Python dependencies. It does not invoke a compiler, Java or graph executable.

The regular Linux CI job now builds one wheel and performs equivalent isolated
checks across 3.12–3.14. Ruff lint/format, ty and diff whitespace checks passed
for this increment. No Rust production source changed, so its suite was not
rerun. Remote CI was not executed or claimed green.

This is Linux x86-64 smoke evidence, not full-suite cross-platform evidence.
The local wheel needs glibc 2.34; it does not establish older manylinux support.
macOS, Windows, AArch64, source-distribution builds, release runner maintenance
and full fixture provenance remain separate outstanding gates. The existing
release workflow was inspected but not altered or triggered. Nothing published.
