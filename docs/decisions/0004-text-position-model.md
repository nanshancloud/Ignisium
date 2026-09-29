# ADR-0004: Text Position Model

## Status

Accepted.

Resolves the open questions listed in ADR-0001.

## Context

ADR-0001 accepted UTF-8 byte offsets "temporarily" and left four questions
open:

- Unicode scalar values
- Grapheme clusters
- Line and column positions
- Visual positions

Week 4 re-examines the offset and range model. The concrete risk is that
"position" means four different things in an editor, and an API that silently
mixes them produces bugs that only appear with non-ASCII text.

## Decision

### 1. Byte offset stays the canonical position

`TextOffset` remains a UTF-8 byte offset, and `TextRange` remains a half-open
byte range `[start, end)`. Reasons:

- It is the unit the storage layer and `str` already use, so no conversion is
  needed on the hot path.
- It is cheap to validate: `str::is_char_boundary` is O(1).
- It is stable: a position before an edit point stays valid after that edit.
- Every other coordinate system can be derived from it.

### 2. The other coordinate systems are named, not implemented

| Name | Unit | Status |
|---|---|---|
| Byte offset | UTF-8 bytes | Canonical, implemented (`TextOffset`) |
| Char index | Unicode scalar values (`char`) | Deferred |
| Grapheme index | UAX#29 grapheme clusters | Deferred |
| Line / column | Logical lines, columns | Deferred (needs LineIndex, week 13) |
| Visual position | Pixel / cell position | Out of scope for Core (Layout, week 23) |

They are deferred, not rejected. Nothing in the current code needs them: there
is no cursor (week 15), no selection (week 15) and no line index (week 13).
Introducing them now would freeze a design that has no consumer to validate it,
which is the kind of speculative API the project principles warn against.

### 3. Every public API must state its coordinate system

Names and documentation carry the unit. `TextOffset` is documented as bytes;
a future cursor type must not expose a bare `usize` position. If several
coordinate systems coexist one day, their types must not be interchangeable.

### 4. Invariants and validation are unchanged

From ADR-0003:

- `offset <= len`
- `offset` is a UTF-8 character boundary
- `range.start <= range.end`, both endpoints valid

Violations return `TextError`, never panic. These rules are about byte offsets
and remain correct no matter which other coordinate systems are added later.

## Consequences

- No public API changes this week.
- The gap between byte offsets and what a user perceives as "one character" is
  now documented and covered by a test matrix
  (`crates/ignisium-core/tests/unicode_matrix.rs`), so the gap cannot be
  forgotten silently.
- When cursor movement arrives (week 15), it must operate on grapheme
  boundaries; that will require a UAX#29 implementation behind an internal
  abstraction, per the dependency policy.
- Line / column requires LineIndex first, so column semantics (bytes? scalars?
  graphemes?) stay open until week 13.

## Future

- `byte <-> char index` conversion is cheap to add with `str::char_indices`.
- Grapheme handling needs a Unicode crate (`unicode-segmentation` or
  equivalent). Per the dependency rules it must sit behind an internal
  abstraction, be recorded in `docs/dependencies/`, and never appear in the
  stable public API.
- Normalization (NFC / NFD) affects search and comparison, not offsets. Decide
  when search arrives (week 18).
