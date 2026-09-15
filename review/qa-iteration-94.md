# QA iteration 94: clean-Git crate provenance

Date: 2026-09-15. Baseline: the isolated 860-file source snapshot from QA 93,
plus the crate-provenance verifier added in the live pending tree. No commit was
created in the real repository.

## Clean repository simulation

The isolated source snapshot was initialized as an independent throwaway Git
repository under the ignored `target/` tree. Exactly the 860 candidate files
were committed as `a9ea0a31328cbc877b2cff85d30d811525f9574e`; its `.venv/`
and `target/` remained ignored and `git status` was clean.

From that clean commit:

- `cargo publish -p cindergraph --dry-run --locked` passed without
  `--allow-dirty`;
- two `cargo package -p cindergraph --locked` runs were byte-identical;
- the archive contained 307 files, including Cargo's generated
  `.cargo_vcs_info.json`;
- the existing manifest, content, notice, registry-dependency and compressed
  size checks passed.

Cargo represents a clean package by omitting the optional `dirty` key rather
than writing `"dirty": false`. The generated document identified the exact
throwaway commit and `path_in_vcs`:

```json
{"git":{"sha1":"a9ea0a31328cbc877b2cff85d30d811525f9574e"},"path_in_vcs":"crates/cindergraph"}
```

## Fail-closed provenance gate

`tools/check_crate_notices.py` now requires `.cargo_vcs_info.json`, validates a
40-character lowercase Git SHA-1, requires it to equal the checkout's current
`HEAD`, rejects `dirty: true`, and requires `path_in_vcs` to be exactly
`crates/cindergraph`.

The focused suite has 25 passing crate-package tests. Direct archive checks
accepted the clean throwaway commit and rejected the earlier live-checkout
archive with:

```text
packaged crate was built from a dirty Git tree
```

The broader release metadata, distribution, crate and documentation subset has
96 passing tests. Ruff formatting/lint and `git diff --check` also pass.

The checker and its tests are release tooling rather than wheel/sdist members,
so the QA 92/93 Python artifact hashes remain unchanged. The final clean real
commit will necessarily produce a different crate hash because its
`.cargo_vcs_info.json` must name that final commit; that hash cannot be known
before the candidate is committed.
