# ADR-0012: Edit Pipeline, Claim Totality and Interruptible Reads

## Status

Accepted. **Decisions 1 to 5 and 7 are implemented** (week 11); decision 6 is
half implemented - the contract is written on the trait and asserted in debug
builds, while `TextStore::replace` and the rope's fused override land in week
12 with the long-line work that re-examines the chunk sizes.

`docs/benchmark/edit-batch-week11.md` is the gate for decision 4: 321 µs to
9.6 µs at 1 000 edits, 32 ms to 246 µs at 10 000.

What the code looked like before this landed, kept because Context describes it:
`claim()` added one to an offset the host chose (`edit.rs`), `edit_problems`
was the pairwise loop (`document.rs`), `for_each_in` returned `()`
(`store.rs`, `rope.rs`), `Rope` derived `PartialEq`, and `TextStore::replace`
did not exist.

Where the decisions live now:

| Decision | Code |
|---|---|
| 1 normalization | `edit::normalize`, `edit::Effective` |
| 2 total claim | `Effective::claim` (saturating) |
| 3 one pipeline | `Document::plan`, used by `edit_problems` and `apply_batch` |
| 4 sweep | `edit::conflicts` |
| 5 stoppable read | `TextStore::for_each_in`, `Node::visit_range` |
| 6 `replace` contract | documented on `TextStore::insert` / `delete`, asserted in `Document::apply_edit`; the method itself is week 12 |
| 7 content equality | `impl PartialEq for Rope`, `Document::text_eq` |

Two consequences for a reader who arrives later:

- The week table in ADR-0013 is the schedule for the decisions here; this ADR is
  the decision record, not the plan.
- Decision 1's conflict rule ("two edits that write at the same position are
  rejected no matter how they are spelled") is refined by ADR-0013 decision 2
  for machine-rate writes: two insertions at one offset are ordered, not
  rejected. Both landed in the same week, so the code has one rule -
  "claims intersect **and** they are not both insertions" - and not a rule plus
  an amendment.

Refines ADR-0006 (transaction rules), ADR-0008 (the layout stays invisible) and
ADR-0011 (decision 3, `for_each_in`).

## Context

A full read of `ignisium-core` plus probes against the current suite found four
defects on the edit path, one missing read capability and two contracts that only
exist as comments. The three layer split (positions / store / document) is
unchanged, and that is why the fixes stay small: each defect sits on a seam
between two layers, or inside a rule that has no single owner.

| Finding | Where | Evidence |
|---|---|---|
| `claim()` overflows | `edit.rs` | `edit_problems([insert(usize::MAX,"x"), insert(1,"y")])` panics in debug, wraps in release |
| An empty-range `Replace` has two identities | `edit.rs` | both batch orders of `[insert(1,"X"), replace(1..1,"Y")]` give `"aXYbc"`; written as two `Insert`s the same pair is rejected |
| Batch validation is quadratic | `document.rs` | `effective.iter().take(position)` per edit: 10 000 edits, 50 million comparisons |
| The read cannot stop | `store.rs`, `rope.rs` | `FnMut(&str) -> ()`: a search hit cannot end the scan |
| Equality compares the tree | `rope.rs`, `document.rs` | `#[derive(PartialEq)]` on `Node`: same text, different chunk layout, unequal |
| `Replace` is two store calls | `document.rs` | `delete` then `insert`, held together by a comment |

### 1. `claim()` adds one to an offset the host chose

```rust
TextEdit::Insert { offset, .. } => TextRange::new(*offset, TextOffset(offset.0 + 1)),
```

A lone `Insert(usize::MAX)` does not panic only by accident: `collect_range_problems`
reports `OffsetOutOfRange` first, and a batch of one never runs the pairwise
loop. A batch with two effective edits does run it, and adds one to
`usize::MAX`. ADR-0003 says host input never panics; `usize::MAX` is host input.

### 2. An empty-range `Replace` is both "not an insertion" and "an insertion"

`is_insertion()` answers `false`, so the endpoint check treats it as a range and
the ordering rule treats it as one. `claim()` answers `[p, p)` - empty - so it
intersects nothing and an `Insert` at `p` is accepted next to it, although both
write text at `p`:

```text
text: "abc"
edits: insert(1, "X") + replace(1..1, "Y")

both orders -> "aXYbc"
the same pair as two inserts -> OverlappingEdits { first: 0, second: 1 }
```

The result is deterministic, so this is not an ordering bug. It is a definition
bug: one intent has two spellings and only one of them is checked.

### 3. Everything else holds

Worth recording, because the fixes below must not weaken it: the single
validation entry point (`validate_offset` / `validate_range`), "every single
edit is a one-edit transaction" (ADR-0010), back-to-front application, and
"one transaction, one revision" were all correct and stay as they are.

## Decision

### 1. An edit is normalized before it is validated

| Input | Canonical form |
|---|---|
| `Insert { text: "" }` | dropped (no-op) |
| `Delete { empty range }` | dropped (no-op) |
| `Replace { empty range, text: "" }` | dropped (no-op) |
| `Replace { empty range, text }` | `Insert { offset: range.start, text }` |
| `Replace { range, text: "" }` | `Delete { range }` |
| `Replace { range, text }` | unchanged |

After normalization:

- **N1** every surviving edit has a non-empty claim;
- **N2** an edit that inserts text is an `Insert`;
- **N3** no surviving edit is a no-op.

`TextEdit` itself does not change: it is Stable, its variants are public, and a
caller may build `TextEdit::Replace { .. }` directly. Normalization therefore
cannot live in the constructors (a public variant bypasses them); it is a
crate-private step at the batch boundary that produces a borrowed view
(`Effective<'_>`) without cloning the payload.

`EditProblem.index` keeps pointing into the caller's batch, not into the
normalized list.

Consequence: the conflict rule ("two edits may not claim the same text") now
depends only on where the edits act, never on how they were spelled. Finding 2
disappears by construction instead of by keeping `is_insertion`, `claim` and the
sort key aligned. ADR-0006's tie-break rule 2 (ranges before insertions at the
same start) becomes unreachable for the three current kinds - a delete and an
insert at the same start conflict - but stays documented as the answer for
future kinds, exactly as ADR-0006 already notes.

### 2. `claim()` is total

```rust
TextEdit::Insert { offset, .. } => TextRange::new(*offset, TextOffset(offset.0.saturating_add(1))),
```

The `+1` is a sentinel: an insertion occupies one position so that "insert at p"
and "delete [p, p+1)" conflict. It is an interval used for comparison only,
never a slice index, so saturation cannot corrupt the text. At `usize::MAX` the
claim degenerates to an empty interval, which is harmless because such an offset
is out of range for every real text (a `String` is limited to `isize::MAX`
bytes) and the edit is already reported as `OffsetOutOfRange`.

Defense in depth on top of that: the conflict scan only compares **well-formed**
edits, i.e. those whose endpoints passed `validate_offset`. An edit that is
going to be rejected anyway cannot contribute a second-order error to the
report.

### 3. Both entry points run the same pipeline

```text
Normalize -> Validate (against the snapshot) -> Order -> Apply -> Report
```

| Stage | `edit_problems` | `apply_edits` |
|---|---|---|
| Normalize | yes | yes |
| Validate | yes | yes |
| Order | - | yes (ADR-0006 keys) |
| Apply | - | yes, back to front |
| Report | `Vec<EditProblem>` | `RevisionId` (`AppliedEdits` at W15) |

The first two stages are one implementation, so "which edits are checked" and
"which edits are applied" cannot drift. `apply_edits` still returns the *first*
problem in batch order; `edit_problems` still returns all of them.

Report order is part of the contract, because a host highlights the list:
problems are ordered by `index`; within one index, endpoint problems (start
before end) come before conflict problems, and conflicts are ordered by `first`.
That is the behaviour the current tests pin.

### 4. Overlap detection is a sweep, not a product

Sort the claims by `(start, end)` and walk them left to right, keeping the
claims whose end is beyond the current start. Every such claim conflicts with
the current one. Cost O(k log k + p) for p reported pairs instead of O(k^2)
always; when everything overlaps, the output itself is quadratic, which is
unavoidable.

This closes G4 of `open-design-gaps.md` ahead of week 15, because the sweep is
the natural way to write a loop that currently contains a panic, and touching
that loop twice is more expensive than touching it once. Per the benchmark gate,
`benches/edit_batch.rs` records the pairwise and swept forms at 1 000 and 10 000
edits before the change is accepted as an improvement.

### 5. `for_each_in` can stop

```rust
fn for_each_in(
    &self,
    range: TextRange,
    visit: &mut dyn FnMut(&str) -> ControlFlow<()>,
) -> TextResult<()>;
```

- `ControlFlow::Break` ends the walk; the call still returns `Ok`. A stop is not
  an error, so it must not travel through `TextResult`.
- The contract becomes: the pieces concatenate to `text_in(range)` **unless the
  visitor breaks**.
- `&mut dyn` keeps the trait object safe (ADR-0011).
- The rope propagates the break out of `Node::visit_range` instead of walking
  the remaining chunks.
- `Rope::text_in`, which is built on `for_each_in`, pushes each piece into its
  `String` and returns `ControlFlow::Continue(())`; the trait default calls
  `visit` once and discards the result (`ControlFlow` is `#[must_use]`). So
  `text_in` stays the convenient call.
- `Document::for_each_in` mirrors the signature.

Why this is cheap today and expensive later: `for_each_in` is Experimental and
has no caller outside the crate. Week 13 (LineIndex) and week 18 (Search) are
written against it; a stop-capable signature added after them is a breaking
change in the read path of a released API.

Rejected alternatives:

- `-> bool`: the meaning of the value ("true means stop"?) is not in the type;
  `ControlFlow` states it at the call site and is what `try_fold` and
  `try_for_each` use.
- A separate `find_in`: search must resume from where it stopped, which is a
  filtered visitor over the same traversal, not a second traversal primitive.
- `()` plus a visitor that returns `Result`: an early stop is not invalid input,
  and overloading `TextResult` would make "the range was invalid" and "the
  caller was done" indistinguishable.

### 6. `TextStore::replace` - decided now, fused by the rope later

```rust
fn replace(&mut self, range: TextRange, text: &str) -> TextResult<()> {
    self.delete(range)?;
    self.insert(range.start, text)
}
```

`Document::apply_edit` uses it for `TextEdit::Replace`, so the deduction "the
insert after the delete cannot fail" moves out of `document.rs` into the store,
where it can be stated as a contract, tested once per backend, and overridden by
the rope with a single walk (one validation, one traversal).

Honest scope: this **localises** the invariant, it does not prove it. What the
pipeline actually depends on is a trait contract:

> After `validate_offset` / `validate_range` has accepted the arguments,
> `insert`, `delete` and `replace` do not return `Err`.

Both current backends satisfy it, because validation is their only error source.
It must become an explicit contract rather than a deduction:

- documented on the trait, next to the validation helpers;
- asserted in debug builds in `apply_edit` (a violation is a backend bug, and
  ADR-0010 lets broken library invariants panic);
- checked by the cross-backend conformance suite (G8), which is the only place
  that can hold every backend to it.

Without that contract, a batch can fail after some edits have been applied,
which contradicts ADR-0006's "all or nothing". Today this cannot happen; the
point is to make that a stated property with a test, not an observation.

### 7. Equality is content, not layout

`Rope` derives `PartialEq` over `Node`, so two ropes holding the same text in a
different chunk layout compare unequal. A storage detail is leaking through a
public type, which is what ADR-0008 promised would not happen; `Document<S>`
then derives over `store`, so the leak reaches `Document`.

- `impl PartialEq for Rope` compares `len` and the text, not the tree. `Eq`
  stays: every leaf is valid UTF-8, so content equality is an equivalence.
- `Document` keeps `#[derive(PartialEq)]` and documents the contract: equal
  means same characters, same metadata, same revision. Editing and undoing back
  to the same text leaves two documents unequal, because the revision differs.
  That is a *state* comparison, and it is the useful default for "did anything
  change".
- `Document::text_eq(&self, other: &Self) -> bool` answers the other question:
  same characters, whatever the revision or metadata. Undo/redo, dirty checking
  and "did the formatter change anything" need it, and without it every host
  compares `text()` values, which allocates on a chunked backend.

Cost: comparison is O(n) without allocation; the derived tree comparison was
also O(n), but compared layout. A lockstep chunk comparator that avoids the
materialised string is a week 12 refinement, not a contract change.

## Consequences

- No sequence of host offsets can overflow, in debug or release: the only
  arithmetic on a host-chosen offset is now saturating, and `TextRange::len`
  was already.
- The set of accepted batches changes in exactly one case: two edits that write
  at the same position are rejected no matter how they are spelled. That is the
  intended reading of ADR-0006 rule 3.
- The problem *list* changes in one case, and only by removing noise: an
  empty-range `Replace` whose offset is invalid used to be checked as a range,
  so `edit_problems` reported the same endpoint twice. After normalization it is
  an insertion and reports one problem.
- Normalization gives the pipeline a place to grow: a future edit kind (an
  "insert many", a "move range") is added by extending the canonical forms,
  not by teaching three predicates about a fourth shape.
- Validation drops from O(k^2) to O(k log k + p), so a 10 000-edit batch becomes
  a sort instead of 50 million comparisons - and it is measured, per the gate.
- Search (W18) can stop at the first hit without materialising the rest of the
  range, and the line index (W13) gets the same read.
- `Rope` equality no longer depends on the chunk layout, which restores the
  promise of ADR-0008 for the one public type that broke it.
- The remaining dependency on "validated means infallible" is written down, so
  the next backend is written against a contract instead of an example.

## Breaking changes

| Change | Migration |
|---|---|
| `for_each_in` visitor returns `ControlFlow<()>` | return `ControlFlow::Continue(())` from the closure; `for_each_in(range, &mut \|p\| { ..; Continue(()) })` |
| `Rope` equality is content based | a host that relied on layout equality has to compare `leaf_count` / `longest_leaf` explicitly |
| `TextStore` gains `replace` with a default | custom backends keep compiling; override it only to fuse the two calls |
| `Document::text_eq` added | additive |

## Lifecycle

| Item | Lifecycle | Notes |
|---|---|---|
| `TextEdit` | Stable | unchanged by this ADR; normalization is internal |
| `apply_edits` / `edit_problems` | Stable | same signatures and results, cheaper validation |
| `Document::text_eq` | Experimental | content comparison for undo and dirty checking |
| `TextStore::for_each_in` | Experimental | signature changes now, before any host |
| `TextStore::replace` | Experimental | default implementation, fused by the rope in W12 |
| `Rope` `PartialEq` | Experimental | content equality |

## Future

- **Week 12 (long-line work)**: `RopeTextStore::replace` as one walk; node
  coalescing after a delete (see G10 of `open-design-gaps.md`); the conformance
  test for "validated means infallible"; a lockstep chunk comparator for
  `Rope`'s equality.
- **Week 15**: the Report stage returns `AppliedEdits { revision, map }`
  (ADR-0011); the first four stages are untouched.
- **Week 18**: search is the first caller that breaks out of `for_each_in`; if it
  needs to resume, the resume point is just an offset, so a second call with a
  narrower range is enough - no new traversal primitive.
- If a future edit kind needs "range before insertion" ordering, the tie-break
  rule is already written; normalization must then be extended first.
