# ADR-0006: Edit Transaction Model

## Status

Accepted.

Refines ADR-0003 ("a rejected operation leaves the text unchanged") and
ADR-0005 ("a transaction advances the revision exactly once").

## Context

Weeks 1–5 expose one edit at a time: `insert` and `delete`, each validated,
each applied, each producing one revision. Real editors do not work that way.

| Situation | What the host wants |
|---|---|
| Auto-indent on enter | remove the indentation, insert a newline plus a fresh one |
| Several cursors | one edit per cursor, all in the same user action |
| Reflow / format | many ranges replaced at once |
| Undo group | the edits of one action, replayed or reversed together |
| AI apply / accept | a set of edits that either lands as a whole or not at all |

Three problems appear at once.

1. **Partial application.** If the third of five edits is invalid, the first two
   are already in the text. The user sees half an action, the undo history is
   polluted, and the revision counter cannot describe the result.
2. **Shifting coordinates.** If edits are applied in array order, every later
   offset has to be translated to account for the length change of the earlier
   edits. Callers get this wrong, and multi-cursor editing is where they get it
   wrong most often.
3. **Ambiguous ordering.** Two edits that touch the same position have no
   defined result, so the same batch could produce different text depending on
   the implementation.

ADR-0003 already promised atomicity for a single operation ("a rejected
operation leaves the text unchanged; there are no partial edits"). Extending it
to a batch needs one more promise: **a batch is one operation**.

## Decision

### 1. A batch is written against the text as it was before it started

```rust
pub enum TextEdit {
    Insert  { offset: TextOffset, text: String },
    Delete  { range: TextRange },
    Replace { range: TextRange, text: String },
}
```

Every offset in a batch refers to the state the document had when the batch was
submitted. The caller never has to translate offsets:

```text
text:      "one two three"
batch:     Replace [0, 3)  -> "1"
           Replace [4, 7)  -> "2"
           Replace [8, 13) -> "3"
result:    "1 2 3"          one transaction, one revision
```

Consequence: the coordinates stay meaningful while the batch is being *checked*,
which is what makes checking it before applying possible.

### 2. All or nothing, checked before anything is modified

```rust
pub fn apply_edits(&mut self, edits: &[TextEdit]) -> TextResult<RevisionId>;
pub fn edit_problems(&self, edits: &[TextEdit]) -> Vec<EditProblem>;
```

`apply_edits` validates the whole batch first. If any edit is invalid it
returns the first problem (in batch order) and the text is untouched. Because
validation happens up front, no edit can fail halfway through the application,
so there is nothing to roll back and no copy of the text is needed.

`edit_problems` is the non-mutating form. It answers "which of my edits are
wrong?" with **all** the answers, not just the first — the open item ADR-0003
left for this week. A host can highlight every offending edit, and an editor
API can reject a whole request with a useful message.

### 3. Two edits may not claim the same text

Each edit *claims* an interval:

| Edit | Claim |
|---|---|
| `Delete { [s, e) }`, `Replace { [s, e), .. }` | `[s, e)` |
| `Insert { p, .. }` | `[p, p + 1)` |

Two claims that intersect are a conflict:

```rust
TextError::OverlappingEdits { first: usize, second: usize }
```

An insertion claims one position rather than nothing, so "insert at 3" and
"delete [0, 6)" cannot both be right. Touching ranges do not conflict:
`[0, 2)` and `[2, 4)` may both be deleted.

The escape hatch is `TextEdit::replace`: an edit that removes a range and
inserts at `range.start` is one claim, not two. That is how a host expresses
auto-indent:

```text
text:   "    return 1;"
batch:  Replace [0, 4) -> "\n    "
result: "\n    return 1;"
```

### 4. Application order is defined, not incidental

Edits are applied from the end of the document towards the start, so an edit
never moves the text another edit still has to touch. Ties are broken so the
result depends only on the batch, not on the sort implementation:

1. higher `span().start` first;
2. at the same start, ranges before insertions — so `Delete [2, 3)` together
   with `Insert at 2` would remove the original byte rather than the byte just
   inserted there (this pair is rejected by rule 3 anyway, which keeps the rule
   meaningful for future edit kinds);
3. at the same start and kind, in reverse batch order — because inserting at an
   offset pushes what is already there to the right.

### 5. One transaction, one revision

A successful transaction advances the revision exactly once, however many edits
it contains (ADR-0005). A batch of no-op edits — inserting `""`, deleting an
empty range — changes nothing at all, so it leaves the revision untouched.

No-op edits are dropped **before** validation. An edit with no effect has no
error and no position to protect: a batch never fails because of an edit that
would not have done anything. This keeps one rule true for the whole batch:
what is validated is what is applied.

### 6. Every single edit is a one-edit transaction

```rust
pub fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<RevisionId>;
pub fn delete(&mut self, range: TextRange) -> TextResult<RevisionId>;
pub fn replace(&mut self, range: TextRange, text: &str) -> TextResult<RevisionId>;
```

`replace` with an empty string is a delete; `replace` over an empty range is an
insertion. It goes through `apply_edits`, so single edits and batches cannot
diverge in validation, ordering or revision behaviour.

**Amended by ADR-0010:** `insert` and `delete` do the same. Before that they
called the store directly, which gave the same text but not the same rules: an
empty insert advanced the revision while the equivalent one-edit transaction
did not. Rule 5 ("no work, no revision") now holds for every entry point.

## Consequences

- No partially applied batch can exist, so no undo entry, style run, search hit
  or background result ever observes a half-finished action.
- Multi-cursor editing needs no offset arithmetic in the host: every edit is
  written in the coordinates the user sees.
- `TextEdit` owns its payload (`String`), so a batch can be built, stored,
  replayed (undo, week 16) and moved to another thread without borrowing the
  document. This is what makes `EditTransaction` / `EditSource` / `EditPreview`
  from the plan (P9, AI edits) implementable on top of this type.
- Checking a batch costs O(k log k) for k edits and applying it costs the sum of
  the individual edits — no full-text copy, which matters as soon as the
  document is large.
- Overlapping edits are refused instead of being silently reordered. A host that
  builds batches from overlapping selections has to merge them first; that is a
  correct place for the error to surface.
- `TextRange::len()` now saturates instead of subtracting blindly, so an inverted
  range created by a host cannot panic the library. Ranges arriving in a batch
  are still validated and rejected with `TextError::InvalidRange`.
- The public API grows one error variant (`OverlappingEdits`) and one type
  (`EditProblem`). Both are reported in terms of batch indexes and byte offsets,
  which keeps them meaningful without exposing the store.

## Lifecycle

| Item | Lifecycle | Notes |
|---|---|---|
| `TextEdit` | Stable | the unit undo, redo and AI edits will replay |
| `apply_edits` | Stable | every host edit path goes through it |
| `edit_problems` | Stable | validation UI, formatters, LSP diagnostics |
| `TextError::OverlappingEdits` | Stable | part of the transaction contract |

## Future

- **Undo / redo** (week 16): a transaction is the natural undo unit. Replaying
  it backwards is a second batch built from `TextEdit` values, so the command
  pattern already has a payload type.
- **Line index** (week 13): a transaction shifts the line numbers after it;
  the affected region is the span of the batch, which is one value rather than a
  list.
- **Selection** (week 15): moving a selection across a transaction needs the same
  "before coordinates" rule, so cursor and selection code should reuse this model
  instead of inventing a second one.
- **Preview / rollback** (P9): `TextEdit` is plain data, so an AI or LSP edit
  set can be shown to the user and then applied or discarded as one unit.
- If a future host needs edits that deliberately overlap (for example a search
  and replace over overlapping matches), the batch can be merged into
  non-overlapping `Replace` edits before it is submitted. That is a host-side
  transformation, not a change to this model.
