# ADR-0005: Document Model (Metadata and Revision)

## Status

Accepted.

## Context

Week 5 turns `Document` from a thin wrapper around a `String` into a document
*model*. The plan asks for three parts:

```text
Document
 ├── TextStore
 ├── metadata
 └── revision
```

Two questions have to be answered before the code is written.

**1. Why does the document need a revision at all?**

The offset model is settled (ADR-0004): a `TextOffset` is a UTF-8 byte offset,
and that number is only meaningful together with the text it was computed
against. An insert in front of a stored position moves it. Any feature that
keeps derived data — search results, style runs, cursor positions, undo
snapshots, background lexer output — has to answer "was this computed from the
text I am looking at now?". Without a revision number the only options are to
recompute everything after every keystroke or to guess. The plan lists this as
P5 (`RevisionId`, "系统知道这个结果是基于哪个 Document Revision 产生的"), and it
has to exist before anything can cache against the document.

**2. What belongs in metadata?**

Anything that is not a character but still changes how the core behaves. The
temptation is to add fields "because a real editor will need them": dirty flag,
file name, newline style, encoding, lexer language, fold state. Each of those
either belongs to another module (line index, lexer, application) or has no
consumer yet. Unused options in the model are permanent API.

## Decision

### Document v0.3

```rust
pub struct Document {
    store: StringTextStore,
    metadata: DocumentMetadata,
    revision: RevisionId,
}
```

The three fields have three different jobs, and the separation matters more than
the fields themselves:

| Field | Answers | Owned by |
|---|---|---|
| `store` | What are the characters? | `TextStore` (ADR-0002) |
| `metadata` | How does the host want this document treated? | `DocumentMetadata` |
| `revision` | Which state are the characters in? | `RevisionId` |

### 1. `RevisionId` is a monotonic counter owned by the document

```rust
pub struct RevisionId(u64);   // private field

impl RevisionId {
    pub const INITIAL: Self;  // a new or freshly loaded document
    pub fn as_u64(self) -> u64;
    pub fn next(self) -> Self;
}
```

Rules:

- The counter starts at `RevisionId::INITIAL` (0). A newly created document and
  a freshly loaded one are both "nothing has been cached against this yet".
- **Every successful mutation advances it exactly once.** An edit that changes
  nothing is not an edit.
- **A rejected mutation does not advance it.** Failed operations leave the text
  unchanged (ADR-0003), so the old revision is still the current one.
- Metadata changes do not advance it. Toggling `read_only` changes how the
  document may be used, not what it contains.
- The field is private. Hosts compare and store revisions; they do not invent
  them. `as_u64()` / `From<RevisionId> for u64` exist for hosts that keep
  revisions in their own maps.

All mutating `Document` methods return `TextResult<RevisionId>`: the caller gets
the revision its edit produced and can key its caches on it without reading the
document again. This is a breaking change to `insert` / `delete` from v0.2 and
is accepted at `0.1.0` per the API version policy.

The counter saturates instead of wrapping. Reaching `u64::MAX` edits is not a
realistic scenario, and saturation at least guarantees the counter never returns
to a value an older cache entry already uses.

### 2. Metadata starts with exactly one field

```rust
pub struct DocumentMetadata {
    read_only: bool,
}
```

`read_only` is included because it already has a consumer (read-only views,
generated output) and because it is the reason `TextError` grew a variant that
describes *policy* instead of offset validity:

```rust
TextError::ReadOnly
```

The read-only check lives in `Document`, not in `TextStore`. Offset validity is
part of the offset model and lives on the trait (ADR-0003); whether this
particular document accepts edits is a property of this document, and a store
does not know it.

Explicitly **not** in metadata yet:

| Candidate | Why not |
|---|---|
| newline / encoding policy | belongs with the line index (week 13); putting it in the document would make storage depend on it |
| dirty flag, file name, path | application level (IgnisTE), not core |
| lexer language, fold state | own modules (weeks 21, 22) |

The rule for adding a field: it needs a caller that acts on it today.

### 3. `from_text` is loading, not editing

```rust
pub fn from_text(text: &str) -> Self;   // revision == RevisionId::INITIAL
```

Opening a file is not an edit, so it must not advance the counter. A host that
loads a document and then starts caching can treat revision 0 as "cache empty".

## Consequences

- Positions, search hits, style runs and background results now have something
  to be keyed against, so the first caching feature (search, week 18) will not
  have to invent a versioning scheme.
- `Document` is no longer `Copy`-shaped trivia: it is the unit that later
  modules (line index, undo, selection) will observe, and `revision()` tells
  them whether their derived data is stale.
- Metadata is one boolean. That is intentional, and it is the cheapest possible
  wrong answer: the type exists so future fields have a home, and adding a field
  later is not a breaking change for `Document`'s layout.
- `Document` remains the only place that decides whether an edit is allowed;
  storage backends stay ignorant of policy, which keeps ADR-0002 intact.
- `RevisionId` counts *edits*, not time and not versions of the file. Hosts
  that need to correlate a background result with the text it was computed from
  use it; hosts that need file versions keep their own counter.

## Lifecycle

| Item | Lifecycle | Notes |
|---|---|---|
| `Document.revision()` | Stable | needed by every caching feature |
| `RevisionId` | Stable | opaque handle; representation may change |
| `DocumentMetadata` | Experimental | grows with real callers |
| `read_only` | Stable | small, orthogonal to text |
| `TextError::ReadOnly` | Stable | error taxonomy grows only with real cases |

Removability check: dropping `metadata` and `read_only` would remove one struct,
one error variant and one guard from `Document`. The store, the revision and the
offset model would be untouched.

## Future

- **Snapshots** (background work, P5): when a background task needs the text, it
  will clone at a known `RevisionId` and report the revision it was computed
  from, so the result can be dropped if the document moved on. This is why
  `RevisionId` is opaque: the representation can grow into a version vector
  without changing the API.
- **Line index, undo, selection** (weeks 13–16): each will be invalidated by a
  revision change, which replaces "rescan everything" with "compare one number".
- **Metadata**: newline policy and encoding will arrive with the line index; they
  belong in a separate `LineEndingPolicy` type rather than in
  `DocumentMetadata`.
- **TextEdit transaction grouping** (week 6) builds directly on this: a
  transaction must advance the revision exactly once, which is now testable.
