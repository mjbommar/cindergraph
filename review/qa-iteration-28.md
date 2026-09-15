# QA iteration 28: current source-distribution round trip

Refreshed artifact validation after QA iterations 14–27. The old source-build
evidence predated the graph, provenance and sizeof fixes. Expanded
`tools/smoke_installed.py` to check unevaluated-call non-reachability, discarded
comma values, incomplete global summaries, and XML parsing of PDG GraphML.
The existing CI source/wheel jobs already invoke this script.

## Artifact and isolated install

```bash
export TMPDIR=/home/mjbommar/.cache/cindergraph
uv run --no-sync maturin sdist --out target/qa-sdist-28
uv venv --python 3.12 /home/mjbommar/.cache/cindergraph/sdist-qa-28-venv
uv pip install --no-cache --no-deps --python /home/mjbommar/.cache/cindergraph/sdist-qa-28-venv/bin/python target/qa-sdist-28/cindergraph-0.1.0.tar.gz
/home/mjbommar/.cache/cindergraph/sdist-qa-28-venv/bin/python -I tools/smoke_installed.py
sha256sum target/qa-sdist-28/cindergraph-0.1.0.tar.gz
```

All passed on CPython 3.12.13. The module identity guard confirmed imports from
that environment, not the checkout. No runtime dependencies were installed.
The PEP 517 build provisioned its own build dependencies and used the available
Rust dependency cache; this is not a hermetic or network-free build claim.

Archive SHA-256:
`adedb7ae75de313948bb0cb6de576691b42f75dd9d4d119050dfe8a8e1195c89`.
Baseline is `edf2777` plus pending QA through iteration 27. This turn changes
the external smoke runner only, not Rust or package implementation.

## Source payload and tests from the archive

The following read-only audit matched all 279 selected Rust source, C fixture,
Python package and stub files byte-for-byte against the archive:

```python
from pathlib import Path
import tarfile
root = Path.cwd()
paths = sorted(set(root.glob('crates/*/src/**/*.rs'))
               | set(root.glob('crates/cindergraph/tests/**/*.c'))
               | set(root.glob('python/cindergraph/**/*.py'))
               | set(root.glob('python/cindergraph/**/*.pyi')))
with tarfile.open('target/qa-sdist-28/cindergraph-0.1.0.tar.gz') as archive:
    for path in paths:
        member = archive.getmember('cindergraph-0.1.0/' + str(path.relative_to(root)))
        stream = archive.extractfile(member)
        assert stream is not None and stream.read() == path.read_bytes(), path
print(len(paths))
```

Unpacked the locally generated archive into a fresh cache directory, then ran:

```bash
CARGO_TARGET_DIR=/home/mjbommar/.cache/cindergraph/sdist-tests-28-target cargo +1.88.0 test --manifest-path /home/mjbommar/.cache/cindergraph/sdist-tests-28.Orc189/cindergraph-0.1.0/Cargo.toml --workspace --all-features -q
```

579 tests passed and one was ignored, including the required corpus tests.
This proves the Rust test corpus does not depend on the original checkout for
these gates. The root Python test suite is not claimed to ship in the sdist.
Ruff check/format and ty check on the smoke runner, plus diff whitespace checks,
passed. No full Python-suite rerun, other-platform build, remote CI, publication
or commit this turn. Existing semantic limitations and the broader goal remain
open. Scratch artifacts were retained; nothing was deleted.
