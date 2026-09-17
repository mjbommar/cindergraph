# External facts: how a consumer tells Cindergraph what the source does not say

Decision record for item 7 of
[`../improvement-list-2026-09-16.md`](../improvement-list-2026-09-16.md),
2026-09-17. Status: decided and implemented in the same change.

## The question

A solver front end lifting a C function needs three things the C text never
states: how many bytes a pointer parameter points at (`capacity(dst)`), how
long the string a `char *` parameter points at is (`strlen(name)`), and how
far a bounded unroller may unroll the function's loops (`unroll`). The
consumer that prompted this list invented a structured comment for them
(`// axeyum: capacity(dst) = dst_len`) and parses it on its own side, from
the six lines above each function. Either Cindergraph adopts a documented
form and carries the facts on the exported nodes, or every consumer keeps its
own regular expression and its own placement rule.

## The decision

**Both forms, one grammar, one landing place.** A fact is a (kind, target,
value) triple. Kinds are `capacity` and `strlen`, which take a parameter as
their target, and `unroll`, which is a function-level fact. A value is either
the name of another parameter of the same function or a decimal integer
literal. A consumer supplies facts either as comments in the source or as an
argument to the API; Cindergraph resolves both against the function's
parameter list, reports what it could not attach, and writes what it could on
the function's exported nodes.

### The comment grammar

```
// @cindergraph capacity(dst) = dst_len
// @cindergraph strlen(name) = name_len
// @cindergraph unroll = 8
```

- The marker is `@cindergraph` (an optional colon after it is allowed). The
  consumer's `axeyum:` marker is accepted as an alias, so the twelve vendored
  samples under `tests/fixtures/defects/` work unchanged. Both markers mean
  the same thing and are read by the same code.
- Only `//` line comments are read. A block comment is prose; a mention of
  the grammar inside one (sample `11` has such a mention in its file header)
  is never a fact.
- A fact comment attaches to the **next function definition**: the region it
  may sit in runs from the end of the previous top-level declaration,
  definition or preprocessor directive to the function's first token, so
  `// expect:` lines, blank lines and block comments in between do not break
  the attachment. A fact comment followed by something other than a function
  definition (a prototype, an `#include`, the end of the file) attaches to
  nothing, and is a diagnostic.
- `capacity(p)` and `strlen(p)` name a parameter `p` of that function and
  give a value that is a parameter of that function or a decimal literal.
  `unroll` takes a decimal literal only.
- Whitespace between tokens is free; anything else on the line after the
  value is a malformed fact.

Function scope is the one place this diverges from the consumer's reading,
which treats `unroll` as file-wide. A file has no node to carry a fact on, so
Cindergraph does not offer file scope: sample `09`'s `// axeyum: unroll = 17`
sits above `zero_bug` and lands on `zero_bug`'s `func_def` alone. A consumer
wanting a file-wide default keeps applying it on its side, or passes the fact
through the API for every function.

### The API argument

```python
cg.AnalysisSession(source, facts={
    "assemble_bug": {"capacity": {"dst": "dst_len", "src": "src_len"}},
    "zero_bug": {"unroll": 17},
})
```

The same argument is accepted by `export_graphs`, `export_path` and
`native_graphs`. The value of `capacity` and `strlen` is a mapping from
parameter name to value; `unroll` is a value. A value is a parameter name
(`str`) or an integer (`int`, or a `str` of decimal digits). A function name
that is not defined in the source, or is defined more than once, is a
diagnostic, since a fact attached to the wrong function is a wrong answer.

This form exists for the consumer that has the facts from elsewhere --- a
header, a decompiler's type recovery, a harness that knows its own buffer
sizes --- and should not have to rewrite the source to state them.

### Precedence

**The API wins.** When a comment and the API both give the same (function,
kind, parameter) key, the API's value is the one exported and its
`facts_source` says `api`. This is not a diagnostic: overriding a comment
from a more authoritative source is the API's purpose. Two comments giving
the same key is a diagnostic, and the first is kept.

### Where the facts land

Every fact appears exactly once in the `ast` export and, for a parameter,
again beside each access in the `ops` export:

- `param_decl` nodes carry `facts` (`capacity=dst_len`, or
  `capacity=256,strlen=n` when both apply, always in that kind order) and
  `facts_source`, one entry per fact in the same order (`comment`, `api`, or
  `comment,api`);
- the `func_def` node carries `facts` (`unroll=8`) and `facts_source` for
  the function-level fact;
- `ops` `load` and `store` nodes whose `declared` is a parameter with facts
  carry the same `facts` and `facts_source`, so a solver front end reads the
  capacity obligation next to the access it constrains.

The attribute values are strings, like every other export attribute; a value
that is all digits is a literal, anything else names a parameter (a C
identifier cannot start with a digit).

### What is refused, and how

Every fact that cannot be attached becomes a `Diagnostic` (severity `error`)
on the session, alongside the parser's, and is otherwise absent from the
export --- never a guess, never a silent drop. The span is the comment line
for a comment fact and the function's name for an API fact (or an empty span
at offset 0 when the function does not exist). The cases:

- a parameter the function does not have, as target or as value;
- a target of `capacity` or `strlen` whose type is known not to be a
  pointer (an `int` cannot have a capacity); a target of `unknown` type is
  accepted, because a typedef from a header this snapshot did not see may
  well be a pointer;
- a value parameter whose type is known to be a pointer (a capacity in
  bytes is a scalar), by the same rule;
- an `unroll` value that is not a decimal literal;
- a duplicate key from two comments;
- a marked comment that does not parse, or that is not followed by a
  function definition;
- an API function name the source does not define exactly once.

The module-level functions discard diagnostics, as the reference already
documents for parser diagnostics; that is the documented reason to use a
session. Because an API fact is an argument the caller wrote, the free
functions raise `ValueError` with the diagnostic's message when one cannot be
attached, rather than returning an export that quietly lacks it.

## Why not one form only

A comment grammar alone forces a consumer with facts from a header to edit
the source it was handed, and a consumer of decompiler output has no source
to annotate. An API alone leaves the vendored samples --- and every sample
anyone writes by hand --- carrying facts the analysis cannot see, and every
consumer parsing them with its own regular expression, which is the
situation this item exists to end. One grammar with two front doors costs one
resolver.

## Why not silently accept a fact on a parameter that does not exist

The repository's answers are conservative: a claim it cannot support is
reported as uncertainty, not rounded to a verdict. A capacity on a misspelled
parameter name is exactly a fact the consumer believes was attached and was
not; the consumer's own sanitizer-replayed verdicts would then be wrong on
the axis where soundness is most fragile. The diagnostic is the contract.
