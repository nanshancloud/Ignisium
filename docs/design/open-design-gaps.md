# Open Design Gaps

Audit of the design as it stands after week 9 (rope prototype + ADR-0010
hardening). Companion to the ADRs: an ADR records a decision, this document
records the questions that are **known to be unanswered**, so that a later
week does not discover them as a surprise.

Each gap has the same shape: the situation, why it happens, which week it
starts to hurt, the candidate answers, and whether the answer changes an API
that already exists.

## How to read the table

| Column | Meaning |
|---|---|
| Decision by | The week in which the answer has to exist, not the week it is nice to have |
| Cost of waiting | What it costs if the answer arrives later than that |

## G1 - A range read copies on a chunked backend

**Situation.** `TextStore::text_in` returns text. A contiguous backend
answers with `Cow::Borrowed`, the rope with `Cow::Owned`. Measured on the
10 MiB probe: reading 10% costs 400 ns on `StringTextStore` and 611 us on
`RopeTextStore` - a factor of about 1500, in the one operation a viewport
performs most often.

**Why.** ADR-0008 decided that the trait returns text, never chunks, so that
the storage layout stays behind the boundary (R8). That decision is what makes
the second backend possible at all, and it is also what forces the copy.

**When it hurts.** Week 13 (LineIndex reads ranges), week 23 (layout builds a
viewport from ranges), week 24 (rendering). A 100 MB single line (week 12) is
the worst case: the "line" of a viewport is the whole document.

**Options.**

1. A visitor: `TextStore::for_each_in(&self, range, impl FnMut(&str))`. The
   host decides whether to copy, accumulate or measure. Works for every
   backend; a contiguous backend calls the closure once.
2. A slice list: `chunks_in(&self, range) -> Vec<Cow<str>>`. More flexible,
   allocates a vector, and invites the host to reimplement storage logic.
3. Keep the copy, and make the host read line by line. Fails on a long line
   and does not help a multi-line viewport.

**Recommendation.** Option 1, as an **addition** to `text_in`, not a
replacement: `text_in` stays the convenient call for small ranges, the
visitor is the one that does not copy. Decide in week 10, land before week 13.

**Closed (ADR-0011).** Implemented as `TextStore::for_each_in`; the rope
overrides the default to walk its chunks. Measured at 10 MiB: 548 us to
6.9 us for a 1 MiB range, and no allocation.

## G2 - No revision guard on an edit

**Situation.** `Document::apply_edits(&mut self, edits: &[TextEdit])` takes no
revision. A batch built against revision 5 is applied to revision 9 as long as
its offsets happen to be valid, which is silently wrong text rather than an
error.

**Why.** ADR-0006 modelled a transaction as "a batch against the current
text". Nothing in the API can express "against the text I read".

**When it hurts.** Everywhere a host holds a result for a moment: a search
result that is applied later (week 18), a queued command, a retried edit, an
undo replayed after a background change. The English plan already promises a
`Revision Check` step in its background-work model (section 9); the API
cannot currently perform it.

**Options.**

1. `apply_edits_at(&mut self, revision: RevisionId, edits: &[TextEdit])`,
   returning `TextError::StaleRevision { expected, actual }`.
2. An `EditBatch { at: RevisionId, edits: Vec<TextEdit> }` value that owns the
   revision, applied as `apply_batch(&mut self, batch: &EditBatch)`.
3. A separate `Document::check_revision(revision)` that the host calls first.
   Cheaper to add, but it is a race (check, then apply) and cannot be enforced
   by the type.

**Recommendation.** Option 2. It also gives `apply_edits` a natural place to
return more than a `RevisionId` (see G3) without growing the argument list.

## G3 - An edit does not report where the text moved

**Situation.** `apply_edits` returns `TextResult<RevisionId>`. A host that
holds offsets - a cursor at 100, a selection, a search hit - cannot ask what
happened to them. Week 15 needs this for multi-cursor editing, week 16 for
undo (an inverse edit has to be expressed in the coordinates *after* the
batch).

**Why.** The transaction model was built from the text's point of view
("what changes"), not the host's ("where do my marks end up").

**When it hurts.** Week 15, and every week after it. It is also the reason
undo would have to re-read the text before applying, which is impossible for
edits that arrive from outside (a paste from a background task, a replayed
macro).

**Options.**

1. Return `AppliedEdits { revision, map: OffsetMap }`, where `OffsetMap` is
   built from the applied edits and answers `map(offset) -> Offset` (and
   `map_range`) in O(log n). The edits are already sorted, so the map is a
   sorted vector of (old_start, old_end, new_start, delta) plus a binary
   search.
2. Return the applied edits only (`Vec<AppliedEdit>` with old/new ranges) and
   let the host map. Lower level, more code per host.
3. A separate `Document::offset_map_since(revision)`. Requires the document to
   remember history; that is undo's job (week 16), and it is the wrong place
   for a single transaction.

**Recommendation.** Option 1. It is the only change in this list that is
**cheap now and expensive later**: after a host exists, changing the return
type of `apply_edits` is a breaking change in the hottest API of the crate.

## G4 - Batch validation is quadratic in the number of edits

**Situation.** `Document::edit_problems` compares every edit with every
earlier edit (`effective.iter().take(position)`), so 10 000 edits in one
transaction cost 50 million comparisons. A multi-cursor paste of that size is
unusual but possible; it is also the case that must not take a second.

**Why.** The pairwise form is the direct expression of "no two edits may
claim the same text", and it was written when batches were small.

**When it hurts.** Week 15 (multi-cursor), and any bulk operation (replace
all in a selection of the document, week 18).

**Options.** Sort the claims by start and sweep: overlapping claims are
adjacent in the sorted order, so the check becomes O(n log n) (plus O(n) for
the sort). The error still names the pair.

**Recommendation.** Fix it when `edit_problems` gains its next caller (week
15), together with a large-batch benchmark so that the fix is measured, not
assumed.
## G5 - A snapshot is a deep copy

**Situation.** `Rope` derives `Clone`, and cloning it copies every chunk and
every node. Taking a snapshot for a background task therefore costs O(n), and
`Document` cannot offer a cheap one either.

**Why.** The prototype's nodes are owned values, not shared ones (ADR-0009).

**When it hurts.** Week 18 (search on a snapshot), week 17 (undo that keeps
old states), week 24 (rendering from a stable view while the user types).

**Options.**

1. Persistent nodes: `Node` children in `Arc`, so a snapshot is a pointer
   copy and an edit rebuilds only the path it touches. This is the structural
   change the rope needs to become a production backend, and it changes
   `Rope: Clone` from "deep copy" to "shared".
2. Copy-on-write at the `Document` level with an explicit snapshot type.
3. Leave it: the host clones the whole document and pays O(n) per background
   task.

**Recommendation.** Option 1, if week 10 decides the rope becomes the default
for large documents. It is a change to `Node`, not to the public API, which
is why it can wait for that decision - but it must not wait past week 18.

## G6 - The error types are not ready for evolution

**Situation.** `TextError` is a plain enum, and `EditProblem` is a plain
struct. A host that matches `TextError` exhaustively stops compiling when a
variant is added, so every new error is a breaking change. `EditProblem`
implements `Display` but not `std::error::Error`, so it composes with neither
`?` nor a `Box<dyn Error>` chain.

**Why.** Both types were written for the core's own use, when the only
matcher was the core itself.

**When it hurts.** Continuously: the plan's API version policy allows
breaking changes at `0.x`, and the cost grows with every host. Adding
`#[non_exhaustive]` is cheapest while nothing outside the crate matches on it.

**Options.** Mark `TextError` and `EditProblem` `#[non_exhaustive]`, and
implement `std::error::Error` for `EditProblem` (`Display` and `Debug` already
exist). Do it now: at `0.1.0` with one consumer, it is free.

**Recommendation.** Do it in week 10, before the first external host.

**Closed (ADR-0011).** Both types are `#[non_exhaustive]` and `EditProblem`
implements `std::error::Error`.

## G7 - The rope is exported while the layout is promised to be invisible

**Situation.** `lib.rs` exports `Rope`, and `RopeTextStore::rope()` hands out
`&Rope`. ADR-0008 promises that the storage layout stays behind the boundary.

**Why.** The prototype needed its diagnostics (`leaf_count`, `depth`) for the
week 9 and week 10 measurements, and exporting the type was the shortest path.

**When it hurts.** Week 10, when the public shape of the backend choice is
decided: a public `Rope` forces every future layout change to be argued as a
public API change.

**Options.**

1. Keep `Rope` public and safe (what ADR-0010 made true). Costs: the layout is
   part of the API, and `Rope`'s methods must stay compatible.
2. Make `Rope` crate-private and expose only `RopeTextStore`, with
   capability queries (`chunk_count()`, `depth()`) on the store for the
   measurements. Costs: the teaching value of the type drops, and the roadmap
   loses a public example of the data structure.
3. Keep it public behind a feature flag or a documented `experimental`
   module.

**Recommendation.** Decide in week 10 together with the default backend.
Option 2 is cleaner once the rope becomes the default, because a host then
never needs to name the rope.

**Decided (ADR-0011).** Option 1 for now: the rope stays public and
Experimental, because the string backend stays the default and the type is
where week 11's chunk model is explained. The decision is revisited when the
backend choice is closed.

## G8 - There is no cross-backend conformance suite

**Situation.** "The rope behaves like the string backend" is a unit test
inside `rope.rs` that compares two concrete types by hand. A third backend
(week 11 chunk model, later a piece table) would need the same test written
again, and nothing forces it to be as thorough.

**Why.** With one backend there was nothing to conform to.

**When it hurts.** Week 11, and every time a backend is added or changed.

**Options.** A generic test module: `fn conformance<S: TextStore>(build:
impl Fn(&str) -> S)` invoked once per backend from a small dispatch list, so
that an edit, a range read, an error and a revision are checked identically
for each. Cheap to write now, and it turns "the backends agree" from a claim
into a test that cannot be forgotten.

**Recommendation.** Do it in week 10 or 11, before the third backend exists.

## G9 - Smaller gaps, recorded so they are not lost

| Gap | Note | Handle in |
|---|---|---|
| `StringTextStore` keeps its capacity after a large delete | a document that shrinks stays large in memory | week 28 (resource policy) |
| No `Document::validate_offset` convenience call | a host must combine `len` and `is_char_boundary` and can get the order wrong | week 10, with G6 |
| `RopeTextStore::text` always allocates | a host that wants "the whole text, if cheap" cannot ask; `for_each_in` covers ranges, not the whole text | week 28 |
| No `Rope::chunk_sizes` / memory query | the overhead was measured in week 10 (1.06x) with a counting allocator in the probe, but a host cannot ask a document what it costs | week 28 (resource policy) |
| `Document` cannot be constructed from an iterator of chunks | loading a large file still requires one contiguous `&str` | week 12 (long line loading) |

## Summary

| Gap | Decision by | Changes an existing API | State |
|---|---|---|---|
| G1 range read copies | week 10, land by 13 | additive | **closed**: `for_each_in` (ADR-0011) |
| G2 no revision guard | week 10 | no, additive `EditBatch` | **decided**: shape in ADR-0011, implement by 15 |
| G3 no offset map | week 10 (shape), land by 15 | no, additive `AppliedEdits` | **decided**: shape in ADR-0011, implement by 15 |
| G4 quadratic validation | week 15 | no | open |
| G5 snapshot is a deep copy | week 10 decision, work by 18 | no (`Clone` semantics) | open, until 18 |
| G6 error types not evolvable | week 10 | additive now | **closed**: `#[non_exhaustive]` in ADR-0011 |
| G7 rope exported | week 10 | no, stays public | **decided**: public and Experimental (ADR-0011) |
| G8 no conformance suite | week 10-11 | no | open, week 11 |
| G9 smaller gaps | as listed | mostly no | memory **measured** (rope costs 6%) |

The one item that was cheap now and expensive later was the **return type of
`apply_edits`** (G2 + G3): it is the centre of the host-facing API, and every
week from 15 onwards wants more from it than a `RevisionId`. ADR-0011 answers
that with an additive shape - `EditBatch` carries the revision, `AppliedEdits`
carries the offset map - and leaves `apply_edits` untouched, so no host has to
be migrated.

The remaining gaps all have a week: G4 and G8 before the multi-cursor work,
G5 before the background search, G9 items as listed.

## Relation to the ADRs

- ADR-0006 defines the transaction rules that G2 and G3 extend.
- ADR-0008 defines the read contract that G1 wants to extend.
- ADR-0009 lists the prototype limits that G5 repeats with a schedule.
- ADR-0010 records the hardening that makes G7 a choice instead of a defect.