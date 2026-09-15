# QA iteration 13: source distribution round trip

The first sdist built and installed through an isolated PEP 517 build, but its
resulting wheel failed `tools/smoke_installed.py`: LICENSE was missing. The
archive contained only `crates/cindergraph/LICENSE` and NOTICE, not the root
copies Maturin includes in Python wheel metadata when building from the checkout.
This is why direct-wheel installation alone was insufficient evidence.

Added explicit sdist-only inclusions for root LICENSE, NOTICE and the two
reference pages. Maturin's [configuration reference](https://www.maturin.rs/config.html)
documents the include configuration. The rebuilt archive carries byte-identical
copies of all four files. The isolated source build now produces a wheel that
passes the installed-package smoke checks, including both notice files.

Local commands:

```bash
TMPDIR=/home/mjbommar/.cache/cindergraph uv run maturin sdist --out target/qa-sdist-fixed
TMPDIR=/home/mjbommar/.cache/cindergraph/sdist-qa.ZyxpmQ uv pip install --no-cache --reinstall --no-deps --python /home/mjbommar/.cache/cindergraph/sdist-qa.ZyxpmQ/venv/bin/python target/qa-sdist-fixed/cindergraph-0.1.0.tar.gz
/home/mjbommar/.cache/cindergraph/sdist-qa.ZyxpmQ/venv/bin/python -I tools/smoke_installed.py
```

The environment is CPython 3.12.13 and contains only the installed Cindergraph
distribution. Build dependencies were provisioned separately by the isolated
backend build; the source build still requires Rust and a linker. No cached
Python wheel was reused (`--no-cache`). The Rust dependency cache was available;
this does not claim a network-free or hermetic build.

Fixed sdist SHA-256:
`b66f014e88b13395550365db756b3f5ca2abb3323d9f8e5e4f0bf3f3fbbf61e1`.
The unfixed archive remains under `target/qa-sdist/`; the fixed one is under
`target/qa-sdist-fixed/`. No files were deleted.

CI now builds and installs an sdist with `--no-cache --no-deps` and runs the
same installed smoke check. Local Python/review regression suite: 912 passed,
ten skipped, three optional Joern cases deselected. No Rust production change
this round. Remote CI, other platforms and registry publication were not run.
Baseline remains `edf2777` plus pending QA work; the broader goal stays open.
