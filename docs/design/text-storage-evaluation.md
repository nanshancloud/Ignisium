# Text Storage Evaluation

Week 8 research document. Companion to ADR-0007; see also ADR-0002
(`TextStore` abstraction) and ADR-0004 (byte offsets).

The plan asks for a comparison of `String`, Gap Buffer, Rope, Piece Table and
B-Tree / chunked rope, and for a decision that does not depend on implementing
any of them yet. This document does the comparison and records the two
conclusions that matter for the code: **which backend comes next** and **what
has to change in `TextStore` before a second backend can exist at all**.

---

## 1. What the core needs from a storage backend

Requirements collected from the plan and from the ADRs:

| # | Requirement | Source |
|---|---|---|
| R1 | Positions are UTF-8 byte offsets, validatable cheaply | ADR-0001, ADR-0004 |
| R2 | Edit operations must not panic, and must report a usable error | ADR-0003 |
| R3 | Text size and runtime resources should be decoupled as far as possible | plan section 3 |
| R4 | A single line may be 100 MB without forcing proportional temporary objects | plan section 2.2 |
| R5 | Line index, search, styling and layout must be able to work incrementally | plan W13-W23 |
| R6 | A document can be snapshotted for background work (search, lexer) | plan P4, P5 |
| R7 | The core must build for `wasm32-unknown-unknown` and WASI | plan section 16.4 |
| R8 | A third-party or platform specific facility must not leak into the API | plan sections 6, 22.8 |
| R9 | Undo must not make memory grow linearly with the document | plan W17 |

Two observations shape everything else:

- **R1 and R4 pull in opposite directions.** Byte offsets are cheap in a
  contiguous buffer and awkward in a tree of chunks. A backend that chops a
  long line into chunks must still report byte offsets, so every chunk access
  needs a tree lookup.
- **R7 rules out memory mapping as a requirement.** `mmap` does not exist in
  WASM. A piece table can still be used there, but its "original buffer" has to
  live in memory, which removes one of its main advantages.

## 2. The candidates

### 2.1 `String` - the current backend

`StringTextStore` keeps the whole text in one contiguous `String`.

| Property | Behaviour |
|---|---|
| Append | amortised O(1) |
| Insert / delete in the middle | O(n): the tail is moved |
| Memory | exactly the text, plus slack from the doubling growth strategy |
| Character boundary | O(1) (`str::is_char_boundary`) |
| Snapshot | O(n): the text has to be copied |
| Long line | one contiguous allocation; a 100 MB line means a 100 MB buffer, and growing it reallocates and copies |
| WASM | fine |
| Complexity | none |

For a document of a few megabytes, and for an editor where every keystroke is
an insertion near the end of the text, this is the best possible backend. Its
weakness is not speed, it is that the two costs which matter for large files -
a middle edit and a snapshot - are both proportional to the document size.

### 2.2 Gap Buffer

The text is split into `prefix | gap | suffix`, with the gap holding the
editing position. Insertions and deletions happen in the gap.

| Property | Behaviour |
|---|---|
| Insert / delete near the cursor | O(gap size), effectively O(1) |
| Insert / delete far from the cursor | O(n): the gap has to travel to the edit position |
| Memory | the text plus a fixed gap (commonly 1/8 of the document) |
| Character boundary | O(1) inside prefix and suffix |
| Snapshot | O(n) copy, or O(1) with copy-on-write reference counting |
| Long line | still one contiguous buffer for the line |
| WASM | fine |
| Complexity | small, about 200 lines |

A gap buffer optimises one specific pattern: continuous typing at one position.
It does not help a document that is edited all over the place, and it does not
help the "several cursors" case, which is exactly the case that week 6 made
first class. It also keeps the whole document in one allocation, so R4 is not
addressed.
### 2.3 Rope

A balanced binary tree whose leaves are text chunks and whose internal nodes
store the length of the text below them.

| Property | Behaviour |
|---|---|
| Append | O(log n) plus the new chunk |
| Insert / delete in the middle | O(log n) for the tree, O(chunk) for the copied chunk |
| Memory | the text plus the tree: each leaf costs a `String` header and two pointers, so roughly 1.3x to 2x the text depending on the chunk size |
| Character boundary | O(log n) to find the chunk, O(1) inside it |
| Reading a range | O(log n + range) |
| Snapshot | O(1) when the nodes are immutable and shared (`Rc` / `Arc`) |
| Long line | a long line is one big leaf unless leaves are split by size; R4 requires splitting |
| WASM | fine, pure Rust |
| Complexity | medium: nodes, split, concat, balancing, and the invariants that come with them |

The rope is the structure that answers R3, R5, R6 and R9 at once. The price is
memory overhead and a much more complex implementation than the string backend.
It is the structure the plan names for week 9.

### 2.4 Piece Table

Three parts: the original buffer, a buffer of everything that was inserted,
and a table of pieces `(start, length, which buffer)` that describes the
current text.

| Property | Behaviour |
|---|---|
| Append | O(1) amortised: append to the added buffer, append a piece |
| Insert / delete | O(k log k) in the number of pieces |
| Memory | the original text is kept alive, plus everything that was inserted, plus the piece table |
| Character boundary | O(log k) to find the piece, then O(1) |
| Snapshot | O(1): copy the piece table |
| Long line | the original buffer is one contiguous allocation; a long line still needs chunking |
| WASM | possible, but the "original" cannot be memory mapped, so the advantage of never touching the original text shrinks |
| Complexity | medium to high: the piece table is a vector that is rebuilt often enough to be a problem in practice |

The piece table's distinctive advantage is that the original file never has to
be copied into a new buffer, which matters when the file is memory mapped and
huge. That advantage is real on native platforms and much weaker on WASM, and
it is also a *document source* question (plan P8, `DocumentSource`) rather than
a *storage* question: a piece table can be filled from a `DocumentSource`
whatever that source is.

### 2.5 B-Tree / chunked rope

A rope with wide nodes: instead of a binary tree, a node holds up to dozens of
children, so the tree is shorter and each node fits into a cache line.

| Property | Behaviour |
|---|---|
| All rope properties | with a shallower tree |
| Memory | slightly better than a binary rope, because there are fewer internal nodes |
| Implementation | more complex: the width has to be balanced, and splits are more involved |
| Long line | the natural place to put the "split a long line into chunks" rule of R4 |

This is where the industry has ended up for large documents: a tree of chunks
with a wide branching factor, where the leaves are bounded in size regardless
of how the text is distributed over the lines.

## 3. Comparison

| Property | `String` | Gap Buffer | Rope | Piece Table | B-Tree rope |
|---|---|---|---|---|---|
| Append (R3) | good | good | good | good | good |
| Middle insert (R3) | poor | fair | good | good | good |
| Middle delete (R3) | poor | fair | good | good | good |
| Memory overhead | good | fair | fair | fair | fair |
| Snapshot / background work (R6) | poor | poor | good | good | good |
| Incremental line index (R5) | fair | fair | good | good | good |
| Long single line (R4) | poor | poor | fair | fair | good |
| Byte offset validation (R1) | good | good | fair | fair | fair |
| WASM (R7) | good | good | good | good | good |
| Undo memory (R9) | poor | poor | good | good | good |
| Implementation effort | done | small | medium | medium/high | high |

The table is not a verdict on its own: the interesting column is the last one.
The rope is the only candidate that is good enough in every row and
implementable at all within the project's pace.
## 4. The finding that matters for the code

`TextStore` today cannot host a second backend:

```rust
pub trait TextStore {
    fn as_str(&self) -> &str;   // <-- one contiguous slice
    ...
}
```

A rope, a piece table and a gap buffer all store the text in pieces. None of
them can hand out a `&str` for the whole document without flattening it, which
means: without copying the entire document, or keeping a permanently stale
copy. The trait was written for a contiguous backend (ADR-0002 kept the
coupling concrete on purpose), and it shows.

Consequences for the plan:

1. **Week 9 cannot start with the trait as it is.** The trait has to be
   reshaped first, around operations that a chunked backend can answer: a
   length, a read of a byte range, and edits. `as_str` has to become something
   like a `Cow<str>` for the whole text, or - better - a range read that a
   caller can use piecewise.
2. **The reshape is a breaking change** for the public API. It is cheapest
   now, while the only consumer is the core itself and the crate is at `0.1.0`.
   It would be much more expensive after a demo product starts using it.
3. **The reshape must not leak the backend.** A `Cow<str>` in the public API
   would make every host copy the whole document; a range based API keeps the
   host in control of how much it materialises.

This is the same "shape the boundary before the implementation" rule the
project has followed since week 1, applied one level higher: the boundary has
to be shaped for the implementation that is *planned*, not only for the one
that exists.

## 5. Decision

See ADR-0007. In short:

- `StringTextStore` stays as the first implementation, and stays the default.
- Week 9 reshapes `TextStore` and implements a simplified rope as a second
  backend behind it.
- Week 10 compares the two with the benchmarks from week 7 and decides whether
  the rope becomes the default for large documents.
- The piece table stays a candidate for a later "open a file without copying
  it" step, driven by a `DocumentSource`, not as a replacement.
- The gap buffer is not pursued: it optimises one editing pattern, while the
  multi-cursor case is the one the core is built for.

## 6. What would change this decision

- The week 7 baseline shows that the string backend is already fast enough for
  the target file sizes: then the rope is only worth building as a prototype,
  not as a default.
- The week 12 long line benchmark shows that a single long line is the real
  problem: then chunking rules matter more than the tree shape, and a chunked
  string backend could be enough.
- A host that only ever edits small documents: then the string backend stays
  the only backend, and `TextStore` is reshaped only far enough to keep the
  option open.