# QA iteration 40: refreshed ABI3 wheel matrix and full Python hygiene

Full-tree Python gates passed before artifact work:

```bash
uv run --no-sync ruff format --check python/
uv run --no-sync ruff check python/
uv run --no-sync python tools/gen_native_stub.py --check
```

Expanded the installed-package smoke runner to inspect the shipped stub's 15
native function declarations and reject unrestricted argument lists. It also
checks duplicate-source reachability uncertainty, duplicate-slice rejection,
and duplicate-comparison rejection, alongside existing memory/provenance,
sizeof, JSON/XML, notice and isolated-import checks. This runner is already
called by the CI wheel/source jobs; remote CI was not run here.

Built one release wheel:

```bash
TMPDIR=/home/mjbommar/.cache/cindergraph uv run --no-sync maturin build --release --out target/qa-wheel-40
sha256sum target/qa-wheel-40/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl
```

SHA-256: `e089294342270f571cea62758810e44be0f06e5218c8015ed7550d8d5a767e52`.
Baseline is `edf2777` plus pending QA through iteration 39. The package now
contains the updated stubs and Python guards absent from earlier wheel evidence.

Installed that exact wheel into three fresh environments with no dependencies:

```bash
for version in 3.12 3.13 3.14; do
  envdir="/home/mjbommar/.cache/cindergraph/wheel-qa-40.GoH107/py$version"
  uv venv --python "$version" "$envdir"
  uv pip install --no-deps --python "$envdir/bin/python" target/qa-wheel-40/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl
  "$envdir/bin/python" -I tools/smoke_installed.py
done
```

All passed on CPython 3.12.13, 3.13.12 and 3.14.3. The smoke guard verifies both
Python and native modules resolve beneath each environment's sys.prefix.
Ruff check/format and ty check on the updated smoke runner, and diff whitespace
checks, passed. The runner does not require pytest or NetworkX.

This is a local Linux x86-64 smoke matrix, not full-suite coverage on every
interpreter or validation of macOS/Windows/AArch64. No new Rust algorithm
changes, Rust-suite rerun, full Python-suite rerun, source-distribution refresh,
publication or commit. Scratch environments and wheel retained. Broader goal
and known semantic limitations remain open; changes local/uncommitted.
