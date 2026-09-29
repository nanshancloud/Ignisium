# Text Position Model: Offset and Range Design

Design draft, week 4. Companion to ADR-0004; see also ADR-0001 and ADR-0003.

This document defines what "a position in text" means for Ignisium, which
coordinate systems exist, and how they convert.

---

## 1. Four coordinate systems

Take the string `👨‍👩‍👧` (family emoji: man + ZWJ + woman + ZWJ + girl):

| System | Unit | Value for the sample | What a user thinks |
|---|---|---:|---|
| Byte offset | UTF-8 bytes | `0..18` | nothing |
| Char index | Unicode scalar values | `0..5` | "why is one emoji five characters?" |
| Grapheme index | UAX#29 clusters | `0..1` | "one character" |
| Line / column | logical line + column | line 0, column 0..1 | "one cell" |

And for `"line1\r\nline2"`:

| System | Value |
|---|---:|
| Bytes | 12 |
| Scalars | 12 |
| Graphemes | 11 (`CRLF` is a single cluster, UAX#29 rule GB3) |
| Lines | 2 |

Key point: **bytes, scalars and graphemes are three different answers to "how
long is this text?"**, and only the last one matches what a person sees.

---

## 2. Why byte offset is canonical

Chosen because it is the only unit that is simultaneously:

| Property | Byte offset | Char index | Grapheme |
|---|---|---|---|
| Native to `String` / `str` | yes | no | no |
| O(1) validation | yes (`is_char_boundary`) | no | no |
| Stable across edits before the edit point | yes | yes | yes |
| Derivable from others | trivially | yes | yes |
| Derives others cheaply | — | O(n) scan | needs UAX#29 data |
| Matches user perception | no | no | yes |

The cost is that byte offsets are not user-facing. That cost is paid at the
edges (cursor movement, selection, display), not in the storage layer — which
is exactly the boundary this project wants.

---

## 3. Conversion matrix

| From → To | Method | Cost | Status |
|---|---|---|---|
| byte → char index | count scalars before the offset | O(n) | future API |
| char index → byte | scan `char_indices()` | O(n) | future API |
| byte → grapheme index | UAX#29 segmentation | O(n) + crate | future, needs dependency |
| byte → line / column | LineIndex lookup | O(log n) with index | week 13 |
| column → byte | LineIndex + column unit policy | depends on column semantics | week 13 |

Rule: conversions belong in the layer that owns the extra data (LineIndex owns
lines, a segmentation service owns graphemes). `TextStore` keeps offering bytes
only.

---

## 4. Invariants

1. `offset <= len` (byte length).
2. `offset` is a UTF-8 character boundary.
3. `range.start <= range.end`.
4. Both range endpoints satisfy 1 and 2.
5. Edits never leave the text in a state that violates 1–4: partial writes are
   not possible (ADR-0003).
6. A position stored by a host is only meaningful together with a revision.
   Caching positions across edits without a revision check is a bug. `RevisionId`
   is a later deliverable (week 15 / P5 in the plan).

---

## 5. Validation and errors

Unchanged from ADR-0003:

| Violation | Error |
|---|---|
| `offset > len` | `TextError::OffsetOutOfRange { offset, len }` |
| offset inside a multi-byte scalar | `TextError::NotCharBoundary { offset }` |
| `start > end` or invalid endpoint | `TextError::InvalidRange { start, end }` |

Note the semantics: **`is_char_boundary` is about scalars, not graphemes.**
`👍🏽` (emoji + skin tone) has a valid boundary in the middle (offset 4) even
though the user sees one character. Editing there splits a grapheme. That is
allowed today and is the honest current behavior; a future editing layer may
want to snap offsets to grapheme boundaries for cursor operations.

---

## 6. API sketch (not implemented)

```rust
// Today
pub struct TextOffset(pub usize);        // UTF-8 byte offset
pub struct TextRange { pub start: TextOffset, pub end: TextOffset }

// Candidates, deferred until a consumer exists
pub struct CharIndex(pub usize);         // Unicode scalar values
pub struct GraphemeIndex(pub usize);     // UAX#29 clusters
pub struct Position { pub line: usize, pub column: usize }
```

Naming rule: any public function taking a position must say which unit it
expects, in the type name or in the function name. A bare `usize` position in
a public signature is a design defect.

---

## 7. Unicode pitfalls to keep in mind

| Pitfall | Example | Effect on positions |
|---|---|---|
| NFC vs NFD | `café` as 4 scalars (NFC) or 5 (NFD) | Same visible text, different byte length |
| Combining marks | `a` + `U+0301` | 1 grapheme, 2 scalars, 3 bytes |
| ZWJ sequences | `👨‍👩‍👧` | 1 grapheme, 5 scalars, 18 bytes |
| Emoji modifiers | `👍🏽` | 1 grapheme, 2 scalars, 8 bytes |
| Regional indicator pairs | `🇺🇸` | 1 grapheme, 2 scalars, 8 bytes |
| CRLF | `\r\n` | 1 grapheme, 2 scalars, 2 bytes |
| Line separators | `U+2028`, `U+2029`, `U+0085` | Not `\n`; affects line counting |
| BOM | `U+FEFF` at start | Occupies 3 bytes at offset 0 |
| Tabs | `\t` | 1 byte, variable visual width |
| Surrogates | none in Rust | `char` is a scalar value, so no surrogate pairs |

Rust guarantees valid UTF-8 in `String`, so illegal sequences cannot appear;
the open questions are about *segmentation and perception*, not validity.

---

## 8. Test matrix

`crates/ignisium-core/tests/unicode_matrix.rs` records for each sample:

- byte length
- scalar count
- grapheme count (recorded by hand: the standard library cannot compute it)
- the set of byte offsets that start a scalar

and asserts:

- every recorded boundary is accepted, and inserting there matches slicing
- every non-boundary offset returns `TextError::NotCharBoundary` and leaves the
  text unchanged
- offsets past the end return `TextError::OffsetOutOfRange`
- deleting the whole range empties the document
- scalar count differs from grapheme count for the multi-codepoint samples

Samples: ASCII, CJK, mixed ASCII/CJK, NFC and NFD `café`, combining mark,
single emoji, emoji with skin tone, ZWJ family, regional indicator flag, CRLF,
tab.

---

## 9. Open questions

1. Which crate provides grapheme segmentation, and which internal abstraction
   hides it? (dependency policy: replaceable, never in the stable API)
2. Does `column` count bytes, scalars, graphemes or display cells? Depends on
   what the layout engine needs (week 23).
3. Line ending policy: does Core normalize `\r\n`, treat lone `\r` as a break,
   and how does `U+2028` behave? Decide with LineIndex (week 13).
4. Should `TextOffset` become an opaque handle (no public `usize`) so a future
   non-byte-backed store can keep the same API? Leaning yes, but it costs
   ergonomics; revisit when a second backend lands.
