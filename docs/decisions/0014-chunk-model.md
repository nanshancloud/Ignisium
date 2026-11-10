# ADR-0014: The Chunk Model Stays Behind the Boundary

## Status

Accepted, **design only**. Week 11 asked whether "text is not one contiguous
object" can be an architectural model and whether the host has to know about
chunks. The answer is: chunks are a storage concept, the host keeps seeing
ASCII-safe byte offsets and ranges, and no chunk-shaped type enters the public
API. This ADR records that answer so the next backend does not reopen it.

Refines ADR-0001 (byte offsets are the coordinate), ADR-0008 (the layout stays
invisible) and ADR-0011 (backend choice, `for_each_in`). Schedules the chunk
constant and node coalescing into week 12 (G10).

## Context

Two backends exist: `StringTextStore` (one buffer) and `RopeTextStore` (a tree
whose leaves are already chunks of at most 1 KiB). The chunk terms the plan
listed - `Chunk`, `ChunkId`, `ChunkOffset`, `ChunkLength` - exist implicitly in
`rope.rs` as `Node::Leaf`, `MAX_CHUNK` and local offsets inside `Node::locate`.

The question week 11 has to answer is not "what is a chunk" but **who is
allowed to see one**. Three shapes were considered:

| Shape | What the host sees | Cost |
|---|---|---|
| Chunk-level API (`chunks()`, `ChunkId`, chunk offsets) | the storage layout | one more coordinate system, and every backend has to fake it |
| Range API only (today) | offsets, ranges, a visitor | none for the host; the backend does the walking |
| Erased view (`text_in` only) | one `String` per read | 548 µs per 1 MiB read on the rope, and an allocation per viewport |

The measurements from week 10 decide the last two: `for_each_in` reads 1 MiB of
a 10 MiB rope in 6.9 µs with zero allocation, against 547.6 µs for `text_in`
(`docs/benchmark/rope-week10.md`). A host therefore does not need to see chunks
to read without copying - the visitor gives it the same zero-copy walk, in
pieces whose boundaries it does not have to know.

What a chunk-level API would cost, concretely:

- a second coordinate system next to the byte offset, and ADR-0001 says there
  is one canonical coordinate;
- a stable identity it cannot have: a delete collapses a single-child node and
  week 12 adds sibling coalescing (G10), so a `ChunkId` would be invalidated by
  edits that do not touch its bytes;
- a layout leak through `Rope`, which ADR-0008 promised would not happen and
  ADR-0012 decision 7 has just repaired for equality.

## Decision

### 1. A chunk is a byte range, not a type the host can name

| Term | Meaning | Visibility |
|---|---|---|
| chunk | a contiguous byte range of the text, always on UTF-8 character boundaries | crate internal |
| chunk id | the position of a chunk in the tree | crate internal, not stable across edits |
| chunk offset | an offset inside one chunk | crate internal; the public coordinate is the text offset |
| chunk length | the byte length of one chunk | crate internal; `Rope::longest_leaf` is the observable summary |

A chunk is on a character boundary at both ends, which is what lets
`for_each_in` hand out `&str` and what keeps "no offset ever splits a
character" (ADR-0001) true without a second rule.

### 2. The read boundary has exactly three calls

```text
text()                      the whole text, borrowed if the backend can
text_in(range)              one value, a snapshot on a chunked backend
for_each_in(range, visit)   pieces, no copy, stoppable (ADR-0012 decision 5)
```

A chunk becomes observable only as the argument of `visit`. A caller that
wants "the next piece" writes a visitor; a caller that wants the text writes
`text_in`. No fourth call, and no `chunks()`.

### 3. Chunk size is a backend policy constant, not an API

`MAX_CHUNK` (1 KiB) is chosen by the backend and may change without an ADR
revision, because nothing above the trait can observe it except through
`longest_leaf` (Experimental) and timing. Week 12 re-validates the constant on
a 500 MiB single line, where the chunk count is 500k; if the constant changes,
the week 10 probes are re-run.

### 4. A new backend enters through the conformance suite, not through a new shape

The chunk model describes chunked backends in general, but a third backend (a
piece table, a delta-based store for machine-rate edits) is added by
implementing `TextStore` and passing `tests/conformance.rs` (G8), including
"validated means infallible" (G12). It does not add a variant to `TextStore`,
and it does not need the chunk terms to be public.

### 5. Nothing in this ADR changes code in week 11

The only week-11 deliverables that touch this model are the ones already
scheduled for other reasons: the stoppable read, normalization, and content
equality. Storage-side work (`TextStore::replace`, node coalescing, the chunk
constant) is week 12, where the long-line measurements can judge it.

Rejected: a chunk-aware public API now (layout leak, unstable identity); an
erased `text_in`-only read path (measured 79x slower on the rope); a
`ChunkId`-based cache key (a cache key is an offset plus a revision, and the
revision already exists).

## Consequences

- The public surface does not grow in this dimension: one coordinate system,
  three read calls, and a backend that can be replaced without the host
  noticing.
- `Rope` stays the only type that exposes layout, and only through
  `leaf_count` / `longest_leaf` / `depth`, all Experimental (ADR-0011).
- A host that wants to stream a document to a socket, a viewport or a model
  context writes a `ControlFlow` visitor and never learns what a chunk is
  (ADR-0013 decision 4).
- Week 12 can change the chunk constant and add coalescing without a breaking
  change, which is the point of keeping the number internal.
- The next backend is judged by behaviour, not by shape: the conformance suite
  is the entrance exam.

## Breaking changes

None. This ADR decides what *not* to expose.

## Lifecycle

| Item | Lifecycle | Notes |
|---|---|---|
| `text` / `text_in` | Stable | unchanged |
| `for_each_in` | Experimental | signature fixed in ADR-0012, stoppable |
| `Rope::leaf_count` / `longest_leaf` / `depth` | Experimental | the only observable layout summary |
| `MAX_CHUNK`, `MAX_BRANCHING` | internal | policy constants, re-validated in week 12 |
| chunk terms (`Chunk`, `ChunkId`, chunk offsets) | internal | not part of the API, by decision |

## Future

- **Week 12**: `TextStore::replace` and its rope override; sibling coalescing
  (G10); the chunk constant re-validated on a 500 MiB single line; the shape
  statement at the top of `rope.rs` corrected to "depth is bounded by the
  height at load time, subtrees may differ".
- **Week 13**: shared nodes (`Arc`) make `clone` cheap (G5). The read API does
  not change: `for_each_in` borrows through the `Arc`.
- **If a third backend is added**: it implements `TextStore`, passes the
  conformance suite, and brings its own chunk policy. Public API unchanged.
- **If a host ever needs cost prediction** ("how many pieces will this read
  visit"), the answer is a documented `chunk_hint(range)` on the backend, not a
  chunk view. Not before a host asks.
