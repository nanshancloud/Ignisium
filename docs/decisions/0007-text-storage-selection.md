# ADR-0007: Text Storage Selection

## Status

Accepted.

Resolves the storage question raised by ADR-0002 and the week 8 research
document `docs/design/text-storage-evaluation.md`.

## Context

ADR-0002 introduced `TextStore` so that the storage backend could be replaced
later, and deliberately kept `Document` bound to the concrete `StringTextStore`
because "the trade-offs are not understood well enough yet". Week 8 is the
week that understands them: it compares the candidates against the
requirements the plan has collected over the previous weeks, and the week 7
baseline provides the numbers.

The requirements, in short:

- byte offsets as the canonical position, cheaply validatable (R1, R2)
- text size and runtime resources decoupled as far as possible (R3)
- a single line of 100 MB must not force proportional temporary objects (R4)
- incremental line index, search, styling and layout (R5)
- cheap snapshots for background work (R6)
- WASM and WASI as compilation targets (R7)
- no backend detail in the public API (R8)
- undo memory that does not grow with the document (R9)

The measured baseline (`docs/benchmark/baseline-week07.md`):

| Operation on 100 MiB | Time |
|---|---:|
| append one byte | 39.1 ms |
| insert one byte in the middle | 43.7 ms |
| delete 10% | 4.6 ms |

A one byte edit on a 100 MiB document costs 39 ms, because the contiguous
buffer has to be reallocated and copied. This is the number the decision has
to answer.

## Decision

### 1. `StringTextStore` stays, and stays the default

It is correct, simple, and the fastest option for the document sizes that
matter most (everything up to roughly a megabyte: 186 ns for an append in a
1 KiB document). Replacing it for small documents would trade a real cost
(memory overhead, more code paths) for a benefit nobody can observe.

The backend keeps being the default **until** a second backend proves itself
in the week 10 comparison.

### 2. The next backend is a rope

Among the candidates (String, Gap Buffer, Rope, Piece Table, B-Tree rope) the
rope is the only one that is good enough in every requirement and
implementable at the project's pace:

- it answers R3, R5, R6 and R9 at once;
- it is pure Rust and portable, including to WASM (R7);
- it is the structure the plan already names for week 9;
- the gap buffer optimises continuous typing at one position, which is not the
  pattern the core is built for (week 6 made multi-cursor editing a first
  class case);
- the piece table's main advantage - never copying the original file - is a
  *document source* question (plan P8), not a storage question, and it is much
  weaker on WASM where memory mapping does not exist.

The rope is implemented as a **second implementation of `TextStore`**, not as
a replacement of `Document`.

### 3. `TextStore` has to be reshaped before that

This is the finding that matters most, and it is a consequence of ADR-0002
being right in a way that was not visible yet.

```rust
pub trait TextStore {
    fn as_str(&self) -> &str;   // one contiguous slice
    ...
}
```

A rope, a piece table and a gap buffer store the text in pieces. None of them
can return a `&str` for the whole document without flattening it, which means
copying the whole document or keeping a stale copy. The trait was written for
a contiguous backend, and it shows.

Therefore week 9 starts with the trait, not with the rope:

- `as_str` is replaced by a range read that a chunked backend can answer
  (`fn text_in(&self, range: TextRange) -> Cow<str>` or an equivalent), plus a
  `Cow<str>` for the whole text for the cases that genuinely want it;
- the shape of the trait follows the operations a chunked backend can serve,
  not the operations a `String` can serve;
- the public API must not expose the chunking: a host that asks for a range
  gets text, not chunks, so the rope stays behind the boundary (R8).

This is a breaking change to the public API, and it is cheapest now: the only
consumer is the core itself, and the crate is at `0.1.0`.

### 4. The decision about the default is deferred to week 10, with data

No backend becomes the default on the strength of this document. The week 7
baseline and the week 9 prototype are compared with the scenarios and
thresholds of `docs/benchmark/benchmark-requirements.md`, and the result is
recorded in `docs/benchmark/rope-week10.md`.

The comparison has to be fair: the baseline has to be measured with the same
mitigation the rope gets. The week 7 numbers include an artifact - a
one byte append on a freshly loaded document pays for a reallocation - and
that artifact has to be removed from both sides before comparing, or the
rope will look better than it is for the wrong reason.

### 5. The long line case is a separate problem

R4 (a 100 MB single line) is not solved by choosing a tree. A tree with
one giant leaf is still one giant allocation. The rule that answers R4 is
**chunking by size, not by line**: leaves are bounded in size, and a line may
span many leaves. That rule belongs to the storage model of whichever backend
is used, and it is the subject of week 12.

## Consequences

- Week 9 has a concrete goal: an append and a middle insert must not cost
  O(document size). The week 7 numbers say what "not O(n)" has to beat.
- `TextStore` changes shape in week 9. `Document` keeps its own API, so the
  change is contained in the storage layer, but the public trait changes.
- `Document` keeps the concrete `StringTextStore` for now. The generic
  `Document<S: TextStore>` question from ADR-0002 is still open: with two
  backends, the choice between a generic parameter and a `dyn` object has to
  be made on evidence, not in advance.
- The gap buffer and the piece table are recorded as evaluated and deferred,
  with the reason, so that a later proposal does not have to repeat the study.
- The long line requirement is now stated as a rule about chunk sizes rather
  than as a hope that a tree will fix it.

## Lifecycle

| Item | Lifecycle | Notes |
|---|---|---|
| `StringTextStore` | Stable | stays the default until week 10 decides otherwise |
| `TextStore` trait shape | Experimental | week 9 reshape, breaking change allowed at `0.1.0` |
| Rope backend | Experimental | week 9 prototype, week 10 comparison |
| Piece table backend | Deferred | revisit with `DocumentSource` (plan P8) |
| Gap buffer backend | Rejected | optimises a pattern the core does not target |

## Future

- Week 9: reshape `TextStore`, implement the rope prototype, add the
  `snapshot`, `transaction` and `read_range` scenarios.
- Week 10: compare the two backends, publish `docs/benchmark/rope-week10.md`,
  decide whether the rope becomes the default for large documents and whether
  `Document` becomes generic or dynamic.
- Week 12: the long line benchmark, and the chunk size rule that answers R4.
- Later: a piece table backend for the "open a huge file without copying it"
  scenario, fed by a `DocumentSource`.