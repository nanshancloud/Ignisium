# ADR-0011: Backend Choice, Read Strategy and Edit Batch Shape

## Status

Accepted.

Answers the four questions that `docs/design/open-design-gaps.md` marked as due
in week 10, using the measurements of `docs/benchmark/rope-week10.md`.

## Context

Week 9 left two backends and four open questions: which one is the default, how
a host reads a range from a chunked backend without paying a copy, whether the
rope stays a public type, and what an edit reports besides a revision.

The numbers that decide them:

| Fact | Measurement |
|---|---|
| The rope removes the edit cost | 100 MiB append: 21.5 ms to 2.4 us |
| The rope loses on small documents | 1 KiB append: 200 ns to 1100 ns |
| `text_in` copies on a chunked backend | 10 MiB read of 10%: 300 ns to 548 us |
| Visiting the chunks does not copy | same range with `for_each_in`: 6.9 us, 0 B |
| The rope costs 6% memory, not 100% | 100 MiB: 106.3 MiB against 100.0 MiB |
| An edit needs half the peak memory | 100 MiB middle insert: 108 MiB against 200 MiB |
| Neither backend snapshots cheaply | 100 MiB: 21.5 ms and 26.5 ms |

## Decision

### 1. The default backend stays the string backend

`StringTextStore` remains the default of `Document<S = StringTextStore>`. The
rope is opt-in, through `Document::from_store(RopeTextStore::from_text(..))`.

The crossover is not a constant, and it is not the same for every operation:
the rope wins on appends above 64 KiB and on middle inserts above roughly
64 KiB to 1 MiB, while the string backend wins on range reads at every size.
A size-based automatic switch would therefore be a policy with a tuning
parameter, hidden inside the document type. The backend stays a host decision.

Guidance for a host, written down so it does not have to be rediscovered: a
document that is opened once and edited is fine on the string backend up to
about 1 MiB; a document that is edited repeatedly and grew past 1 MiB should
use the rope.

### 2. `Rope` stays public and Experimental

It is exported at the crate root and documented as Experimental. The
alternative - making it crate-private and exposing the rope only through
`RopeTextStore` - was considered and rejected for now, because:

- week 11 builds the chunk model on the same shape, and the type is the place
  that work is explained;
- the diagnostics (`leaf_count`, `depth`, `longest_leaf`) are what the tests
  and the benchmarks assert on;
- since ADR-0010 a host cannot make it panic, so a public export is not a
  hazard, only a compatibility promise.

This is revisited when the backend question is closed (a `DocumentKind` or an
automatic choice). At that point "internal" becomes the cheaper option, because
a host would no longer need to name the rope.

### 3. A range read that does not copy is the one to use

`TextStore::for_each_in(range, visit)` visits the pieces of a range without
materialising it. It is additive: `text_in` keeps its meaning and every backend
gets a correct default implementation that calls `text_in` once.

```rust
fn for_each_in(&self, range: TextRange, visit: &mut dyn FnMut(&str)) -> TextResult<()>;
```

`&mut dyn FnMut` and not `impl FnMut`, so that the trait stays object safe.

**Amended by ADR-0012:** the visitor returns `ControlFlow<()>`, so a scan can
stop at its first hit without materialising the rest of the range:

```rust
fn for_each_in(
    &self,
    range: TextRange,
    visit: &mut dyn FnMut(&str) -> ControlFlow<()>,
) -> TextResult<()>;
```

The change is made while the method is Experimental and has no caller outside
the crate; after week 13 and week 18 are written against it, it would be a
breaking change in the read path.

The rope overrides it to walk the chunks, which replaces the copy with one
closure call per chunk: 548 us become 6.9 us at 10 MiB, and 5.5 ms become
144 us at 100 MiB.

Guidance: a viewport, a search scan and the line index of week 13 use
`for_each_in`; `text_in` stays the convenient call for a range whose text is
needed as one value.
### 4. An edit batch carries its revision and reports where the text moved

The shape is fixed now, and implemented before week 15:

```rust
pub struct EditBatch {
    at: RevisionId,
    edits: Vec<TextEdit>,
}

pub struct AppliedEdits {
    revision: RevisionId,
    map: OffsetMap,
}

impl Document<S> {
    pub fn apply_batch(&mut self, batch: &EditBatch) -> TextResult<AppliedEdits>;
    pub fn apply_edits(&mut self, edits: &[TextEdit]) -> TextResult<RevisionId>;
}
```

- `EditBatch::at` is the revision the offsets were computed against. A batch
  built on an older revision is rejected with
  `TextError::StaleRevision { expected, actual }` instead of being applied to
  text it does not describe.
- `OffsetMap::map(offset)` answers where an offset moved: a position before an
  edit keeps its distance, a position at or after an edit shifts by the
  difference, and a position inside a replaced range maps to the start of the
  replacement. The edits are already ordered, so the map is a binary search.
- `apply_edits` stays as it is and is defined in terms of
  `apply_batch(EditBatch::at(self.revision(), edits))`. Both are additive, so
  deferring the implementation costs nothing - unlike a change to the return
  type of `apply_edits`, which is why the shape was decided now and not in
  week 15.

**Amended by ADR-0013:** `EditBatch`, `apply_batch`, `AppliedEdits` and
`TextError::StaleRevision` are pulled forward from week 15 to **week 11**,
because an asynchronous writer needs the revision guard before it needs
multi-cursor. The shape above does not change, and the map lands with the guard
rather than after it, so `apply_batch` has one signature for its whole life.
Week 15 consumes the map (multi-cursor, undo); it no longer creates it.

Reason for the shape rather than a second `apply_edits_at` method: one
argument list cannot grow, and an edit transaction that knows its revision is
also the value undo needs to record (week 16).

### 5. Error types are ready for evolution

`TextError` and `EditProblem` are `#[non_exhaustive]`, and `EditProblem`
implements `std::error::Error`. Adding a variant or a field is now a minor
change instead of a breaking one, and a host can put a problem into a
`Box<dyn Error>` chain.

This had to happen before the first external host, not after.

## Consequences

- A host has two documented ways to read a range and knows which one is for a
  viewport.
- The next five weeks (11 to 15) do not have to reopen the read API, the
  backend choice or the transaction return type.
- The rope's 6% memory overhead and the 46% lower peak per edit are measured
  numbers, so "the rope costs too much memory" can no longer be argued without
  data.
- `EditBatch` and `OffsetMap` are specified but not implemented yet. They are
  additive, so the cost of landing them in week 15 is the same as today; the
  cost of *changing* `apply_edits` later would not be.

## Lifecycle

| Item | Lifecycle | Notes |
|---|---|---|
| `StringTextStore` | Stable | default backend |
| `Rope` | Experimental | public, safe for any host input, wins on large edits |
| `RopeTextStore` | Experimental | opt-in backend for large documents |
| `for_each_in` | Experimental | additive read without copying; stoppable visitor since ADR-0012 |
| `EditBatch` / `AppliedEdits` | Experimental | decided, to be implemented by week 15 |
| `TextError` / `EditProblem` | Stable | `#[non_exhaustive]` from now on |

## Future

- Week 11 builds the chunk model; `Rope` stays the reference implementation of
  the shape.
- Week 13 uses `for_each_in` for the line index, and measures the line scan on
  the rope with it.
- Week 15 uses `EditBatch` and `OffsetMap` for the multi-cursor work (both land
  in week 11 with the guard, per ADR-0013), and records the cursor mapping rule
  next to `OffsetMap` instead of letting every host invent it.
- Snapshot sharing (ADR-0009) remains the one place where the prototype is
  behind; it becomes urgent in week 18, not before.