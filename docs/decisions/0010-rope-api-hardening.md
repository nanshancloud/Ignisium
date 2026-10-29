# ADR-0010: Rope API Hardening and a Single Edit Engine

## Status

Accepted.

Corrects the prototype boundary recorded in ADR-0009, and the no-op rule of
ADR-0006. Triggered by the code review of 2026-10-27.

## Context

The week 9 review found five issues in `ignisium-core`. Two of them were
rated P0:

1. `Node::locate` computes `children.len() - 1` without a check, so an empty
   internal node would panic. No constructor produces one today, but nothing
   says so.
2. `Rope` is a public type whose positional methods do not validate their
   input. `Rope::delete(TextRange::new(TextOffset(1), TextOffset(2)))` on
   `"你好"` panics inside `str::split_at`, which contradicts ADR-0003.

Re-measuring the review's other claims changed two of them:

- The claim that `Node::delete` degenerates to O(n) is wrong: the recursion
  only descends into children the range touches, so the cost is already
  proportional to the deleted text. The missing early exit is a constant
  factor, not a complexity bug.
- The claim that the two edit entry points are merely "inconsistent in style"
  understates it: `insert(offset, "")` advanced the revision while
  `apply_edits(&[insert(offset, "")])` did not. ADR-0006s rule "no work, no
  revision" was only enforced on the transaction path.

The review is the trigger, not the question. The question is architectural:
**where does validation belong when a backend has both a public data type and
a store adapter, and what kind of failure is a panic allowed to be?**

## Decision

### 1. Two kinds of failure, two mechanisms

| Failure | Mechanism | Example |
|---|---|---|
| The host passed something invalid | `Err(TextError)` | an offset inside a character |
| The library broke its own invariant | `assert!` | an internal node with no children |

A panic is a programming error, never a way to reject input. The corollary is
what makes both halves cheap: a public method validates its input **once**, at
the boundary; after that, internal code may assert freely, because the input
is already known to be good.

This is why `locate` keeps an `assert!` instead of returning `Option`. An
empty internal node is not a state a caller can produce or recover from;
`Option` would push a branch that can never be taken into every call site.

### 2. Validation belongs to the type that owns the layout

`Rope` knows the chunk layout, so `Rope` validates. `RopeTextStore` becomes a
thin adapter that forwards and does not validate again:

```rust
impl TextStore for RopeTextStore {
    fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<()> {
        self.rope.insert(offset, text)
    }
    // ...
}
```

The rules themselves are defined once, in the storage-neutral part of the
text layer:

```rust
pub(crate) fn validate_offset(len: usize, boundary: bool, offset: TextOffset) -> TextResult<()>;
pub(crate) fn validate_range(len: usize, range: TextRange, is_boundary: impl Fn(TextOffset) -> bool) -> TextResult<()>;
```

`TextStore` default methods and `Rope` both call them, so a backend cannot
drift from the position model of ADR-0001, and the `TextError` variants stay
identical across backends by construction.
### 3. Every positional `Rope` method is fallible

| Method | Before | After |
|---|---|---|
| `Rope::insert` | `()` - panics on a bad offset | `TextResult<()>` |
| `Rope::delete` | `()` - panics on a bad range | `TextResult<()>` |
| `Rope::split_at` | `(Rope, Rope)` - panics | `TextResult<(Rope, Rope)>` |
| `Rope::text_in` | `String` - panics on a bad range | `TextResult<String>` |

`Rope::concat` stays infallible: it takes no position. `Rope::text`, `len`,
`is_empty`, `is_char_boundary`, `leaf_count`, `depth` and `longest_leaf` stay
infallible because none of them can fail.

The rule is not "make everything fallible" but: **a method that accepts a
position from the caller is fallible; a method that only reports about the
structure is not.**

### 4. Invariants are checked where they are created

`Node::internal` is the only constructor of an internal node, so it asserts
that the child list is not empty. `Node::locate` documents the invariant and
asserts it again for the reader. Both assertions are unreachable from a host,
because `Rope` validates first; they exist to catch a broken constructor, not
broken input.

### 5. Loading is not editing

`Document::from_text` used to run the fallible, validating insert path and
then `expect` that it could not fail. That was an implicit coupling to
`StringTextStore`s current validation rules. Loading is now infallible by
construction:

```rust
StringTextStore::from_text(&str) -> Self   // a &str is valid UTF-8, nothing to validate
Document::from_text(&str) -> Self          // no Result, no expect
```

A future backend rule (BOM handling, encoding detection) cannot turn a
constructor into a panic.

### 6. One edit is one transaction

`insert`, `delete` and `replace` are now one-edit transactions:

```rust
pub fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<RevisionId> {
    self.apply_edits(&[TextEdit::insert(offset, text)])
}
```

There is one validation path, one ordering rule, one no-op rule and one
revision rule, instead of two that can drift.

**Behaviour change:** a single edit that changes nothing no longer advances
the revision. `insert(offset, "")` and `delete(empty range)` return the
current revision, exactly like the equivalent one-edit transaction. This is
the fix of the P2 finding, not a side effect.

### 7. A delete keeps the tree tight

Two changes to `Node::delete`:

- The walk stops as soon as it passes `range.end`, so the cost is
  proportional to the deleted text plus the path to it.
- A node left with a single child is collapsed. A large delete would
  otherwise leave a chain of levels that branch nowhere.

## Consequences

- `Rope` can no longer be made to panic by any position a host can pass. A
  test loops over every offset pair of a multi-byte text and asserts that the
  rope stays consistent.
- `Rope`s four positional methods are a breaking change. Accepted at `0.1.0`,
  same policy as the ADR-0008 reshape.
- `RopeTextStore` lost its validation code: it is now a pure adapter over a
  backend that enforces its own invariants.
- The no-op rule of ADR-0006 now holds for every entry point, not only for
  batches.
- No performance change was measured. The 10 MiB probe reports a delete of
  10% at 106.8 us against 112.6 us before the early exit; append and middle
  insert are unchanged within noise.
- `Document::from_text` cannot panic, and cannot be broken by a future change
  to a backend validation rule.

## What is still open

- **Does `Rope` stay public?** It is exported at the crate root, which is in
  tension with ADR-0008s promise that the storage layout is invisible to
  hosts. Week 10 decides between "public and safe" (what this record makes
  true) and "internal, with `RopeTextStore` as the only surface". If the rope
  becomes the default backend for large documents, the second option is the
  cleaner one, because a host then never needs the rope type.
- **Merging underfull nodes** remains a production concern (ADR-0009). The
  prototype collapses a node that is left with one child, which removes the
  empty levels a delete creates, but a chain of two-child nodes is still
  possible.
- **`split_at` and `concat`** are still the teaching versions (O(n)). Making
  them structural is a separate change with its own measurements.

## Lifecycle

| Item | Lifecycle | Notes |
|---|---|---|
| `Rope` | Experimental | now safe for any host input; public export still under review |
| `RopeTextStore` | Experimental | thin adapter over `Rope` |
| shared `validate_offset` / `validate_range` | Internal | one definition for every backend |
| `Document::insert` / `delete` / `replace` | Stable | one-edit transactions |
| `Document::from_text` | Stable | infallible loading |

## Future

- Week 10 decides the default backend and the public shape (generic
  parameter, `dyn TextStore`, or enum), and with it whether `Rope` stays
  exported.
- A production rope shares its nodes for snapshots (ADR-0009) and merges
  underfull nodes; both are structural changes to `Node`, not to the API this
  record hardens.