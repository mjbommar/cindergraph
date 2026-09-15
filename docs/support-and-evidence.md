# Support and evidence

This page describes the current pre-alpha support boundary. It deliberately
separates source-tree behavior, local artifact evidence, configured workflows
and publication. A check mark in one category does not imply the others.

## Runtime and language boundary

| Surface | Intended boundary | Current evidence | Important limit |
| --- | --- | --- | --- |
| Rust core | Rust 1.88+, no Python dependency, unsafe code forbidden | Workspace and exact packaged-crate tests passed on local Linux in [QA 70](https://github.com/mjbommar/cindergraph/blob/main/review/qa-iteration-70.md) | Candidate came from a dirty tree and used available dependency caches |
| Python | CPython 3.12–3.14, private ABI3 extension | The exact normalized manylinux 2.17 wheel passed isolated runtime and its independently rebuilt archive was byte-identical in [QA 92](https://github.com/mjbommar/cindergraph/blob/main/review/qa-iteration-92.md) | The wheel runtime was exercised locally on 3.12; ABI3 and sdist checks cover 3.13–3.14, but remote platforms remain unverified |
| Python sdist | Build with Rust toolchain and PEP 517 backend | The exact candidate built, installed and passed the installed-artifact smoke suite on CPython 3.12, 3.13 and 3.14 in [QA 92](https://github.com/mjbommar/cindergraph/blob/main/review/qa-iteration-92.md); its `graphs` extra also passed in an isolated 3.12 source install | Not hermetic or network-free; only local Linux exercised |
| Optional graphs | NetworkX through the `graphs` extra | The exact local wheel and sdist installed their extras into isolated environments and passed runtime smoke; wheel consumers also passed downstream type checking in [QA 92](https://github.com/mjbommar/cindergraph/blob/main/review/qa-iteration-92.md) | NetworkX is optional and not needed for core import or serialized export; remote platform runs remain unverified |
| C input | Ordinary and decompiler-shaped C, tolerant partial recovery | Bundled fixture, mutation and serialization suites exercise the documented fragment | Not a compiler, preprocessor, full C semantic model, CPG or general Joern implementation |

The root package currently has no mandatory Python runtime dependency. Native
analysis does not require Java, Graphviz or a C compiler. A compiler is useful
as a test oracle for C semantics but is not part of package execution.

## Platform release matrix

| Platform | Release workflow | Local artifact evidence in this checkout | Release claim |
| --- | --- | --- | --- |
| Linux x86-64 | Configured for manylinux 2.17 on Ubuntu 24.04 | The current tree's normalized manylinux 2.17 wheel was built twice in the pinned Maturin 1.15.0 container, was byte-identical, and passed exact-artifact smoke tests in [QA 92](https://github.com/mjbommar/cindergraph/blob/main/review/qa-iteration-92.md) | Current tree still needs the hosted workflow build; no registry publication |
| Linux AArch64 | Configured for manylinux 2.17 on native Ubuntu 24.04 ARM runner | None recorded | Unverified |
| macOS x86-64 | Configured on macOS 15 Intel runner | None recorded | Unverified |
| macOS arm64 | Configured on macOS 15 ARM runner | None recorded | Unverified |
| Windows x86-64 | Configured on current Windows runner | None recorded | Unverified |
| musllinux | Not configured | None | Not supported |
| PyPy / free-threaded CPython | Not configured | None | Not supported |

The release workflow installs and smoke-tests each generated wheel and the
sdist before uploading them as workflow artifacts. Publication is tag-gated:
the core waits at a protected `crates-io` environment and uses a scoped Cargo
token, then Python artifacts wait at a protected `pypi` environment and use
PyPI trusted publishing. The matrix configuration began in
[QA 15](https://github.com/mjbommar/cindergraph/blob/main/review/qa-iteration-15.md),
but no successful remote matrix run is
recorded here. Workflow configuration is not evidence that either environment
or credential exists, nor that either registry has received a release.

The first crates.io release uses a protected-environment, narrowly scoped
bootstrap token because crates.io cannot attach a trusted publisher before a
crate exists. That is not the steady-state target: after the first release, the
operator must configure OIDC trusted publishing, enable its trusted-only policy
and revoke the bootstrap token before the next release.

The locked Rust graph is checked against current RustSec advisories and an
explicit licence/source/duplicate-dependency policy in both CI and release
validation. These are point-in-time database checks: a later advisory can
change the result without any source change.

On 2026-09-14, strict pip-audit 2.10.1 found no known vulnerabilities in the
hash-pinned dependency set exported from `uv.lock` for all published Python
extras. The local Cindergraph project is deliberately omitted because it is not
yet on PyPI; unlike an installed-environment audit, this route has no skipped
package. CI and release validation repeat the same export and audit.

## Registry-name snapshot

On 2026-09-15, unauthenticated `GET` requests to the official
[PyPI project API](https://pypi.org/pypi/cindergraph/json) and
[crates.io crate API](https://crates.io/api/v1/crates/cindergraph) both returned
HTTP 404. The configured
[GitHub repository](https://github.com/mjbommar/cindergraph) returned HTTP 200.

This establishes only that neither registry exposed a published project under
the exact name at that moment. It does not reserve the names, prove account
ownership, configure trusted publishing or guarantee that a first upload will
succeed. Repeat both registry requests immediately before preparing the release
tag, then verify ownership through the authenticated registry interfaces.

The same refresh found no configured GitHub deployment environments. The
local `main` was five commits ahead of `origin/main` (`edf2777` versus
`28ae864`), and the newest hosted CI result covered only `28ae864`. Therefore
the protected `crates-io` and `pypi` environments, their approval and
credential policies, and hosted validation of the pending release tree remain
release blockers rather than inferred configuration.

## Semantic confidence

Positive analysis results describe dependencies found by the implemented
model. Negative results require the corresponding completeness signals to be
true, and even then apply only to the documented fragment—not arbitrary C.

- Parser and CFG recovery diagnostics matter. Several convenience APIs return
  only their projection, so call `analyze()` as well when recovery status is
  material.
- `recovery_free`, `effects_complete`, summary `complete`, and
  `memory_complete` answer different questions. `effects_complete` fails closed
  for opaque value-producing builtins; `vla_complete` separately exposes
  missing type-level provenance through VLA typedefs. None is a general proof
  of soundness.
- Includes, macros after preprocessing, opaque typedef expansion, global side
  effects and unrestricted aliasing remain incomplete. Direct local and outer
  parameter VLA bounds are tracked, including the evaluated VLA-type exception
  inside `sizeof`; ordinary `sizeof` and `_Alignof` operands remain
  unevaluated. `_Generic` selection inside a bound fails closed because this
  token layer cannot establish C type compatibility. Provenance through a VLA
  typedef likewise fails `vla_complete`. Direct assignment and increment
  writes in bounds kill prior values, but their token-level sequencing also
  keeps `vla_complete` false.
- Declarator-scoped pointer depth and array rank exclude initializer syntax;
  the QA 49 boundary defect was repaired and regression-tested in
  [QA 56](https://github.com/mjbommar/cindergraph/blob/main/review/qa-iteration-56.md).
  Parameter groups also balance nested function signatures, but a lone
  identifier from an unprocessed header remains ambiguous when no preceding
  file-scope typedef declaration is available. Block-scope typedef identity is
  tracked lexically, but typedef target types are not generally resolved.
- General CFGs and parity CFGs are intentionally different representations.
  Parity CFGs model the narrow graph shape used by an offline comparison, not a
  Joern API or code property graph.

See [Python source analysis](reference/source-python.md) for per-API behavior
and uncertainty handling.

## What is still required before a public release

1. Select and commit a release snapshot; current evidence was gathered across
   a dirty series of local QA increments.
2. Run all Rust, Python, formatting, lint, typing, stub and documentation gates
   against that exact snapshot and a freshly rebuilt release extension.
3. Build and test the crate, wheel and sdist from that snapshot, recording
   hashes and testing the packaged contents rather than the checkout.
4. Obtain green remote results for every declared platform, then download and
   inspect the exact workflow artifacts.
5. Repeat the dated registry-name check above, verify ownership, review
   licensing/provenance, and make publication an explicit human-approved
   operation.
6. After the first crates.io publication, migrate the next release to its
   short-lived OIDC trusted-publishing action and revoke the bootstrap token.

Until those steps are complete, use source revisions or local artifacts by
identity and do not describe Cindergraph as published or generally production
ready.

Migrating Glaurung from its embedded implementation to Cindergraph remains
important follow-up work, but it is not a first-release gate: requiring a
consumer to use a registry artifact before that artifact exists would make the
release sequence circular. The migration boundary and its separate acceptance
criteria are documented in [Relationship to Glaurung](architecture/glaurung.md).
