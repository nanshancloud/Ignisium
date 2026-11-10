# Week 11: Batch Validation Before and After the Sweep

The decision record is ADR-0012 (decision 4), the gate is G4 of
`docs/design/open-design-gaps.md`. This document is the data behind both.

## Environment

| Item | Value |
|---|---|
| Machine | Windows x64, consumer CPU |
| Profile | `release` (`cargo bench`) |
| Backend | `StringTextStore` (the validation stage is backend independent) |
| Input | ASCII, `count` one-byte replacements spaced four bytes apart |
| Batch sizes | 1 000 and 10 000 edits |
| Statistics | Criterion, `--quick`, 10 samples, document and batch built before the timer |

Reproduce:

```text
cargo bench --bench edit_batch
```

`pairwise` is the shape the code had before week 11 (`O(k²)`: every pair of
claims compared). It is kept in the bench file as a reference implementation,
because the library no longer contains it. `sweep` is `Document::edit_problems`
today: normalize, validate, sort the claims, walk them once.

## Validation

| Edits | Pairwise (before) | Sweep (after) | Speed-up | Comparisons |
|---:|---:|---:|---:|---:|
| 1 000 | 321.0 µs | 9.58 µs | **33x** | 0.5 M → ~10 k |
| 10 000 | 31.97 ms | 245.8 µs | **130x** | 50 M → ~100 k |

The old form is quadratic and the new one is `O(k log k + p)`, so the gap grows
with the batch: 33x at one thousand edits, 130x at ten thousand. At ten
thousand edits the old loop alone took a whole 60 Hz frame, which is what a
token stream produces in about five seconds.

## Apply

`Document::apply_batch` - normalize, validate, order, apply back to front, and
build the offset map:

| Edits | Time | Per edit |
|---:|---:|---:|
| 1 000 | 54.1 µs | 54 ns |
| 10 000 | 3.267 ms | 327 ns |

The map costs one entry per edit and is built while applying, so the batch does
not walk the edits a second time. What dominates at ten thousand edits is the
backend, not the pipeline: each replacement moves the tail of the `String`.

## What this settles

| Question | Answer |
|---|---|
| Is validation still quadratic | no: a sort and one sweep |
| Does a 10 000-edit batch fit in a frame | yes: 246 µs of validation, 3.3 ms of apply on the string backend |
| Does the offset map cost a second pass | no: it is built from the edits the apply loop already holds |

## What is still not measured

- A batch whose edits all overlap: the report itself is quadratic there
  (`p = k²/2`), which ADR-0012 records as unavoidable.
- The same batch on the rope backend, where the per-edit apply cost is much
  lower (week 12, with the long-line work).
- Batches above ten thousand edits (`EditBatch` has no size limit, but a flush
  policy at that size is host territory).
