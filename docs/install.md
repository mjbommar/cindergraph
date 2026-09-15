# Install and build Cindergraph

Cindergraph is not yet published on PyPI or crates.io. Install the release
candidate from GitHub, or use the checkout and artifact workflows below. Pin
the Git revision and record artifact hashes when reproducibility matters.

## Requirements

- CPython 3.12, 3.13 or 3.14 for the Python package;
- Rust 1.88 or newer for source builds;
- `uv` for the repository's locked Python environment;
- a platform C linker required by Rust/PyO3 builds.

NetworkX is optional. Core parsing, metrics, serialized graph export and
dataflow do not require it. Install the `graphs` extra only for adapters that
return NetworkX objects.

## Install from GitHub

Add the Python package to a uv-managed project:

```bash
uv add "cindergraph @ git+https://github.com/mjbommar/cindergraph.git@main"
```

Include the optional NetworkX adapter with:

```bash
uv add "cindergraph[graphs] @ git+https://github.com/mjbommar/cindergraph.git@main"
```

Installation from GitHub builds the native extension from source and therefore
requires Rust and a platform linker. For a temporary environment:

```bash
uv venv
uv pip install "cindergraph @ git+https://github.com/mjbommar/cindergraph.git@main"
uv run python -c 'import cindergraph as cg; print(cg.analyze("int f(void){return 1;}").functions[0].name)'
```

Rust projects can add the core crate directly:

```bash
cargo add cindergraph --git https://github.com/mjbommar/cindergraph.git
```

Use a disk-backed temporary directory for builds:

```bash
export TMPDIR="$PWD/target/tmp"
mkdir -p "$TMPDIR"
```

## Develop from the checkout

Create the locked development environment and build the native extension in
release mode:

```bash
uv sync --locked --dev
uv run --no-sync maturin develop --release
uv run --no-sync python -c 'import cindergraph; print(cindergraph._native.__file__)'
```

Rebuild with `maturin develop` after every Rust change. Use `--release` before
performance measurement; debug and release timings are not comparable. The
printed extension path confirms which binary Python loaded, which matters when
an older extension remains in the package directory.

Run the ordinary local gates with:

```bash
cargo +1.88.0 test --workspace --all-features
cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings
cargo +1.88.0 fmt --all -- --check
RUSTDOCFLAGS='-D warnings' cargo +1.88.0 doc --workspace --no-deps
cargo audit --deny warnings
cargo deny check
uv run --no-sync pytest python/tests/ review/test_design_contracts.py
uv run --no-sync ruff format --check python/ tools/
uv run --no-sync ruff check python/ tools/
uv run --no-sync ty check python/
uv run --no-sync python tools/gen_native_stub.py --check
```

These commands test the current tree. They do not prove that a generated
wheel, sdist or crate contains the same files.

The security commands require current `cargo-audit` and `cargo-deny` binaries.
CI installs a locked `cargo-audit` version and uses a commit-pinned
`cargo-deny` action; neither an old local advisory database nor a previous
green run is evidence about the current lockfile.

## Build and test a wheel

```bash
uv run --no-sync maturin build --release --out target/wheels
python3 tools/normalize_wheel_sbom.py target/wheels/cindergraph-*.whl
python3 tools/check_python_distribution.py target/wheels/cindergraph-*.whl
uvx --from twine==7.0.0 twine check --strict target/wheels/cindergraph-*.whl
uv venv --python 3.12 target/venvs/wheel-smoke
uv pip install --no-deps \
  --python target/venvs/wheel-smoke/bin/python \
  target/wheels/cindergraph-*.whl
target/venvs/wheel-smoke/bin/python -I \
  tools/smoke_installed.py
uv venv --python 3.12 target/venvs/wheel-graphs
wheels=(target/wheels/cindergraph-*.whl)
test "${#wheels[@]}" -eq 1
wheel="${wheels[0]}"
uv pip install --python target/venvs/wheel-graphs/bin/python \
  "${wheel}[graphs]"
target/venvs/wheel-graphs/bin/python -I \
  tools/smoke_graphs_installed.py
typing_dir="target/typing-consumer"
mkdir -p "$typing_dir"
cp tests/typing/pyproject.toml tests/typing/consumer.py \
  tests/typing/graphs_consumer.py "$typing_dir/"
uv run --no-sync ty check --project "$typing_dir" \
  --python target/venvs/wheel-smoke "$typing_dir/consumer.py" \
  --error-on-warning
uv run --no-sync ty check --project "$typing_dir" \
  --python target/venvs/wheel-graphs "$typing_dir/graphs_consumer.py" \
  --error-on-warning
sha256sum target/wheels/cindergraph-*.whl
```

Run the smoke checker from outside the checkout when manually investigating
import ownership. It rejects Python/native modules that do not resolve beneath
the target virtual environment. `--no-deps` demonstrates the core package's
dependency-free Python runtime; it intentionally does not test the optional
NetworkX adapter.

The local Linux artifact recorded in the support matrix is ABI3 for CPython
3.12+. ABI3 reduces the wheel count but does not imply PyPy, free-threaded
CPython, operating-system or architecture compatibility.

## Build and test a source distribution

```bash
uv run --no-sync maturin sdist --out target/sdist
python3 tools/check_python_distribution.py target/sdist/cindergraph-*.tar.gz
uvx --from twine==7.0.0 twine check --strict target/sdist/cindergraph-*.tar.gz
for version in 3.12 3.13 3.14; do
  smoke_env="target/venvs/sdist-$version"
  uv venv --python "$version" "$smoke_env"
  uv pip install --no-cache --no-deps --python "$smoke_env/bin/python" \
    target/sdist/cindergraph-*.tar.gz
  "$smoke_env/bin/python" -I tools/smoke_installed.py
done
uv venv --python 3.12 target/venvs/sdist-graphs
sdists=(target/sdist/cindergraph-*.tar.gz)
test "${#sdists[@]}" -eq 1
uv pip install --no-cache \
  --python target/venvs/sdist-graphs/bin/python \
  "${sdists[0]}[graphs]"
target/venvs/sdist-graphs/bin/python -I \
  tools/smoke_graphs_installed.py
sha256sum target/sdist/cindergraph-*.tar.gz
```

Installation from the sdist invokes an isolated PEP 517 build and therefore
needs Rust, a linker and provisioned build dependencies. `--no-cache` prevents
reuse of a previously built Cindergraph wheel; it does not make the build
network-free or hermetic.

## Use the Rust crate from a sibling checkout

For local development against a sibling checkout, use an explicit path
dependency:

```toml
[dependencies]
cindergraph = { path = "../cindergraph/crates/cindergraph" }
```

The pure Rust crate exposes `csource`, `syntax`, `dataflow`, `export`,
`metrics`, `normalize`, `parity` and `parse`. The PyO3 binding crate is an
implementation detail of the Python distribution and should not be a Rust
consumer dependency.

For current platform qualifications and remaining release gates, see
[support and evidence](support-and-evidence.md).
