# ADR-0013: Transfer Semantics for Machine-Rate Edits

## Status

Accepted. **Decisions 1, 2 and 4 are implemented** (week 11, together with
ADR-0012): `EditBatch` / `apply_batch` / `StaleRevision` / `AppliedEdits` /
`OffsetMap`, same-offset insertion ordering, and the stoppable read.
Decision 3 (one batch per flush, session in the host) is a policy for the
adapter and lands in week 12 with it; decision 5 (shared nodes) in week 13;
decision 6 is a documented requirement from week 12 on.

Where the decisions live now:

| Decision | Code |
|---|---|
| 1 revision guard + map | `EditBatch`, `Document::apply_batch`, `TextError::StaleRevision`, `AppliedEdits`, `OffsetMap` |
| 2 same-offset insertions | `edit::conflicts` (claims intersect and are not both insertions) |
| 3 flush policy | host side, week 12 |
| 4 stoppable transfer read | `TextStore::for_each_in` (ADR-0012 decision 5) |
| 5 shared nodes | week 13 |
| 6 machine-rate edits on the rope | adapter requirement, week 12 |

Refines ADR-0006 (transaction rules) and ADR-0011 (backend choice, `EditBatch`
shape), keeps ADR-0012 decision 5 (`ControlFlow`) as the read half, and
reschedules G2 and G5 of `docs/design/open-design-gaps.md` while registering
G13 there.

## Context

The edit path was designed for a human writer. An AI client changes the shape of
the write, not its content:

| Assumption | Human writer | Machine writer |
|---|---|---|
| Rate | 5 to 20 edits per second | one edit per token, 20 to 2000 per second |
| Origin | local, immediate | remote, asynchronous, retried |
| Ordering | strict: the user sees what was typed | best effort: a socket stalls, resends, reorders |
| Turn size | a keystroke, a paste | a paragraph, a whole file, per turn |

Nothing in the storage model breaks under that change. Byte offsets as the
canonical coordinate (ADR-0001), atomic transactions (ADR-0006) and a monotonic
revision (ADR-0005) are what a remote writer needs: no unit conversion at the
protocol boundary, one natural alignment point for a retransmission, no ABA
problem for a cache. What breaks is the **time** dimension. Every transfer-facing
interface is written as if "now" is the only moment an edit can refer to, and as
if an edit arrives alone.

### Four defects, verified against the code at the end of week 10

**1. An edit carries no revision.** `apply_edits(&mut self, edits: &[TextEdit])`
(`document.rs`) returns a revision and takes none. A batch built against r5 is
applied to r9 as long as its offsets happen to be valid. That is silently wrong
text instead of an error, which is what G2 of `open-design-gaps.md` recorded
with the same wording. For a local writer the window is a few instructions wide
and the mistake is a programming error; for an asynchronous writer it is the
normal case.

**2. Two insertions at one offset are a conflict.** `claim()` gives an insertion
the interval `[p, p + 1)` (`edit.rs`) so that "insert at p" and "delete
[p, p + 1)" conflict, and two `Insert`s at `p` therefore conflict as well. A
token stream writes every token at the same logical cursor, so a host has two
options: concatenate the tokens into one `String` and re-create the batch per
flush, or submit one edit per token and pay defect 3.

**3. One edit is one revision.** Every entry point routes through
`apply_edits`, so one edit is one transaction is one revision (ADR-0010). A
1000-token answer produces r1 to r1000. Everything keyed on a revision -
viewport invalidation, undo grouping, a host's merge step - is then invalidated
1000 times for one thing the user perceives as one change.

**4. The read cannot stop.** `for_each_in` is the right shape for a transfer
(chunked, zero copy, the visitor can write straight to a socket), but the
visitor returns `()`, so a client that disconnected or a context window that is
already full still scans the whole range. ADR-0012 decision 5 has the answer;
this ADR only records that the transfer path depends on it.

### The backend consequence

Week 7 and week 10 measured both backends on the same operations
(`docs/benchmark/baseline-week07.md`, `docs/benchmark/rope-week10.md`):

| Operation | String 100 MiB | Rope 100 MiB |
|---|---:|---:|
| append one byte | 39.1 ms (week 7) | 2.4 µs |
| middle insert one byte | 43.7 ms (week 7) | 4.2 µs |

43.7 ms per token is a ceiling of about 23 tokens per second, with the editor
frozen for the whole time. The crossover the same measurements give is about
64 KiB for appends and between 64 KiB and 1 MiB for middle inserts. ADR-0011
("string by default, rope opt in") stays right for the documents it was decided
for; it was decided for human-rate editing.

### What does not change

- Byte offsets stay canonical: a protocol has nothing to convert.
- A transaction stays atomic: it is the retransmission unit a network needs.
- `RevisionId` stays monotonic and saturating: no wraparound, no ABA.
- `edit_problems` still reports the whole batch in one call: a pre-flight check
  over the wire.
- The layout stays invisible to the host (ADR-0008).
- The crate keeps zero runtime dependencies and a `&mut self` single writer.

## Decision

### 1. A batch names the revision it was written against (week 11)

The shape is the one ADR-0011 already fixed; this decision changes when it is
implemented, not what it is.

```rust
pub struct EditBatch {
    pub at: RevisionId,
    pub edits: Vec<TextEdit>,
}

pub struct AppliedEdits {
    pub revision: RevisionId,
    pub map: OffsetMap,
}

impl<S: TextStore> Document<S> {
    pub fn apply_batch(&mut self, batch: &EditBatch) -> TextResult<AppliedEdits>;
}
```

- A stale batch is `TextError::StaleRevision { expected, actual }`, a new
  variant. `TextError` is `#[non_exhaustive]` (ADR-0011), so the addition is
  not a breaking change.
- The revision is checked **before** validation: a batch written against a state
  that no longer exists cannot be judged by its offsets. The pipeline order
  becomes `writable -> revision -> normalize -> validate -> order -> apply`.
- `apply_edits` keeps its signature and its meaning ("against the current
  text"). It is the local single-writer path and the one the existing tests
  pin. A host that receives edits from outside uses `apply_batch` and treats
  `StaleRevision` as "the state I based this on is gone".
- `EditBatch` carries **no id**. A retransmission identity belongs to whatever
  retransmits: the core would have to store it or hand it back for it to be
  useful, and the transport already has one. Keeping it out is what makes the
  boundary in decision 7 real.
- `AppliedEdits` lands with the guard rather than in week 15 (G3). The two are
  one question - a host that gets `StaleRevision` has to choose drop or rebase,
  and the map *is* the rebase - and the map is cheap here: the pipeline has
  already ordered the claims, so it is a sorted vector and a binary search over
  edits the batch is holding anyway. Landing the guard with a `RevisionId`
  return and the map later would change the return type of a new API for no gain.
- The map is not the only answer: a host is free to drop a stale batch and
  re-read. `apply_batch` rejects first, which keeps the map optional rather than
  mandatory.

### 2. Two insertions at one offset are ordered, not rejected (week 11)

This refines ADR-0006 rule 3 and the sentence in ADR-0012 decision 1 that says
two edits writing at the same position are rejected however they are spelled.
That sentence is right for an insertion against a range and wrong for two
insertions.

| Pair | Result |
|---|---|
| `Insert(p, "a")` + `Insert(p, "b")` | accepted, text becomes `"ab"` at `p` |
| `Insert(p, ..)` + `Delete(p..q)` / `Replace(p..q, ..)` | still `OverlappingEdits` |
| `Insert(p, ..)` + a range that starts before `p` and covers it | still `OverlappingEdits` |

Implementation is one predicate, not a new edit kind: `claim()` keeps the
`[p, p + 1)` sentinel (it is what makes insertion against range conflict), and
the conflict test becomes "the claims intersect **and** the two edits are not
both insertions". The application order already produces the right text: the
sort key of ADR-0006 applies edits back to front and, at an equal start,
descending in batch index (`document.rs`), so two insertions at `p` are applied
last-first and read as batch order. A test pins `["a", "b"]` to `"ab"` and
`["b", "a"]` to `"ba"`.

Why not "the host concatenates the tokens": it can, and that stays the cheapest
answer for a single cursor, but it forces the host to own a text buffer whose
granularity it then also has to define for undo. Accepting the pair keeps the
core's batch the only place where "what an edit did" is decided.

### 3. A stream is one batch per flush, and the session lives in the host (week 12)

The core guarantee is already the right one: one batch, one revision. No new
API.

- A streaming session is host state: `{ pending: Vec<TextEdit>, at: RevisionId }`
  flushed on a deadline (a frame, 16 ms), on a token budget, or before any read
  that must observe the text.
- Consequence: the revision count is O(flushes), not O(tokens). A 1000-token
  answer submitted from a 16 ms flush loop is between 1 and about 30 revisions
  instead of 1000, and undo granularity, viewport invalidation and host merge
  all key on the flush, which is the granularity a reader perceives.
- Rejected: a core session object (`begin` / `push` / `commit`). It would put a
  timer and a buffer policy inside a zero-dependency library, it cannot be
  shared with the second writer (`&mut self`), and it duplicates what a `Vec`
  and a deadline already say.

### 4. The transfer path uses the stoppable read (week 11)

No API of its own; it depends on ADR-0012 decision 5.

- A viewport, a context window or a file upload is streamed with `for_each_in`,
  never with `text_in` above one chunk: at 10 MiB, `text_in` copies 1 MiB in
  547.6 µs where `for_each_in` visits it in 6.9 µs and allocates nothing.
- The visitor returns `ControlFlow<()>` so that a closed socket, a satisfied
  client or a full context window ends the walk. Without it the transfer path
  would either scan ranges it does not need or re-implement chunk logic in the
  adapter, which is the one thing ADR-0008 forbids.

### 5. Snapshots share nodes (week 13, pulled forward from week 18)

G5, moved because it is on the transfer critical path rather than only on
search's: "send the document out" is the highest-frequency operation of a
session, and today every round trip pays O(n) - 21.5 ms at 100 MiB on the string
backend, 26.5 ms on the rope, both for a copy that shares nothing.

- `Node` holds its children in `Arc`. An edit rebuilds the path it touched and
  copies nothing else; `Rope::clone` becomes a pointer copy.
- No public API change: `Rope: Clone` keeps its signature and changes its cost.
  A clone is still isolated from later edits (copy on write), so a host that
  relied on "clone is an independent document" is unaffected.
- `for_each_in` stays zero copy: it borrows through the `Arc`.

### 6. Machine-rate edits run on the rope (week 12)

- ADR-0011 is unchanged: the string backend stays the default, the rope stays
  opt in, and the measured crossover (about 64 KiB for appends, up to 1 MiB for
  middle inserts) stays the threshold for human-rate editing.
- What this ADR adds is the requirement: **a document above 1 MiB that receives
  machine-rate edits must be rope-backed.** Below the crossover the string
  backend is faster for both, and a small document never notices.
- The core does not choose. `Document<S>` is generic over its backend
  (ADR-0008), so the adapter that owns the connection chooses, and the choice is
  a documented requirement of that adapter with a benchmark gate in week 12
  rather than a heuristic inside the library.
- Rejected now: a size-based default constructor in core (a `Document::open`
  that returns a different `S`). It needs an erased or enumerated backend and it
  would fix a policy that belongs to the host. Revisit when a second host wants
  the same threshold.

### 7. The core stays a library and the transport stays outside

- Zero dependencies, no I/O, no sockets, no timers, `&mut self` single writer:
  unchanged. These are what make the core embeddable, testable and the reason a
  host can put any transport in front of it.
- Not now: CRDT or OT. There is no concurrent edit to merge - `&mut self` means
  one writer at a time, and the asynchronous case produces a *stale* edit
  (decision 1), not a concurrent one. Revision guard plus an `OffsetMap` rebase
  (G3) covers "AI writes and human writes" for the cases in view; a CRDT would
  charge every operation for a problem the type system already prevents.

## Consequences

- An edit that arrives late is an error instead of wrong text. Code that ignores
  the error keeps compiling and starts seeing `Err`, which is the intended
  failure mode.
- The accepted set changes in one place: two insertions at one offset become
  valid. Combined with ADR-0012's normalization, the rule becomes "two edits may
  not consume the same text", which is what ADR-0006 rule 3 was trying to say.
- A batch of 10 000 streamed edits is a sort (ADR-0012 decision 4) instead of a
  quadratic scan, which matters because a flush can be large.
- Revision growth becomes a property of the flush policy, so a host can tune it
  without touching the core.
- `apply_batch` returns the revision and the offset map from its first release
  (ADR-0011's shape), so no host sees a second signature for the same call.
- A 1000-token answer into a 10 MiB document costs 4.85 s of frozen editor on
  the string backend and 2.4 ms of insertion on the rope; the backend choice
  stops being a micro-optimisation and becomes a correctness-of-experience
  property.
- Snapshot cost drops from O(n) to O(depth) of pointer copies at week 13, which
  is what makes a per-turn context send affordable.
- The core still has no notion of a connection, a retry or a client, so the
  transport can be tested without it and it can be tested without a transport.

## Breaking changes

| Change | Migration |
|---|---|
| `TextError::StaleRevision` added | none: `TextError` is `#[non_exhaustive]` |
| Two insertions at one offset are accepted | a host that depended on the rejection merges or splits the batch itself; no signature changes |
| `Rope: Clone` costs O(1) and shares nodes | none; behaviour is unchanged because clones are copy on write |

## Lifecycle

| Item | Lifecycle | Notes |
|---|---|---|
| `EditBatch`, `Document::apply_batch` | Experimental | the asynchronous write path |
| `AppliedEdits`, `OffsetMap` | Experimental | pulled forward from week 15 with the guard |
| `TextError::StaleRevision` | Experimental | `non_exhaustive`, so it may change shape |
| Same-offset insertion ordering | Experimental | becomes part of ADR-0006's rules when ADR-0012 lands |
| `Rope` shared nodes | internal | no API change; `Clone` cost only |
| Backend requirement for machine-rate edits | policy | documented on the adapter, measured by its benchmark |

## The plan

The week table below is the schedule this ADR adds to
`docs/design/open-design-gaps.md`; each row has a gate, so "done" is a
measurement and not a claim.

| Week | Work | Gate |
|---|---|---|
| 11 | ADR-0012 decisions 1 to 5 and 7; `EditBatch` + `apply_batch` + `StaleRevision` + `AppliedEdits` (G2, G3); same-offset insertion ordering; cross-backend conformance suite (G8) | a stale batch is rejected before validation; two insertions at one offset read in batch order; mapping a hit through a batch costs O(log n); the 10 000-edit sweep is benchmarked against the pairwise loop |
| 12 | flush policy in the adapter; `TextStore::replace` and its rope override; rope as the documented backend for machine-rate documents at or above 1 MiB; long lines | tokens per second on both backends at 1, 10 and 100 MiB; revisions per 1000-token stream |
| 13 | node persistence (G5), pulled forward from week 18 | cloning a 100 MiB document under 1 µs, no allocation |
| 15 | multi-cursor and undo consume `OffsetMap`: an inverse edit expressed in post-batch coordinates | a 1000-cursor batch maps in one pass |
| 18 | search: the first caller that breaks out of `for_each_in` | a scan stops at the first hit |

## Future

- If a batch needs an id for deduplication, it is added to the host's message
  envelope, not to `EditBatch`; revisit only if two independent hosts both need
  the core to deduplicate.
- If a third edit kind appears ("insert many", "move range"), normalization
  (ADR-0012 decision 1) is where it is taught, and this ADR's conflict rule is
  expressed in terms of claims, so it does not change.
- If a second writer ever becomes possible, the revision becomes a vector or a
  version pair; that is the point at which OT/CRDT stops being premature. Not
  before.
