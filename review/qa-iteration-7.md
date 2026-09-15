# QA iteration 7: Rust archive notices

Baseline: `edf2777` plus pending iteration-6 documentation work. Inspecting the
actual Cargo archive reproduced missing LICENSE and NOTICE files. The SPDX
manifest field was present, but neither file was shipped.

Added package-local copies of both root files. The root LICENSE now has a final
newline; its wording is unchanged. `tools/check_crate_notices.py` reads the
generated archive without extracting it and requires one regular root-level
copy of each file, byte-identical to the repository originals. CI invokes it
after packaging, deriving the version from Cargo metadata. Copies therefore
cannot silently drift from the authoritative root files.

Local evidence on Rust 1.88:

- `cargo +1.88.0 package -p cindergraph --allow-dirty --offline --no-verify`
  followed by the new checker failed on the original missing LICENSE.
- After adding the files, `cargo +1.88.0 package -p cindergraph --allow-dirty
  --offline` built and verified the archive. The checker also caught a terminal
  newline difference; after normalizing that newline, both files matched.
- `uv run python tools/check_crate_notices.py
  target/package/cindergraph-0.1.0.crate`: passed.
- `cargo +1.88.0 test --manifest-path
  target/package/cindergraph-0.1.0/Cargo.toml --offline -q`: 572 passed,
  one ignored, executing the packaged core rather than the workspace core.
- Ruff lint/format and ty on the new tool passed.

Cargo's [manifest documentation](https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields)
distinguishes SPDX metadata from a licence file. This check concerns delivery
of the existing project notices, not a legal review or complete fixture audit.
Python wheel/sdist notices and multi-platform installation remain separate
artifact gates. No registry publication or remote CI run occurred.
