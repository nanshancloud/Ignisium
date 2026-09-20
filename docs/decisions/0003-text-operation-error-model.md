# ADR-0003: Text Operation Error Model

## Status

Accepted.

Supersedes the note about offset validation in ADR-0002 (Consequences).

## Context

In v0.1 `TextStore::insert` and `TextStore::delete` were infallible. Offsets were
passed straight to `String::insert_str` / `String::replace_range`, which panic
when an offset is past the end of the text or lands inside a UTF-8 multi-byte
character:

```rust
document.insert(TextOffset(1), "x"); // "你好" -> panic
```

A panic is not an acceptable failure mode for an embeddable editing component:

- it takes down the host application (a GUI editor, a CLI tool, a language
  server) for what is usually a recoverable mistake;
- it cannot be handled, logged or translated at the API boundary;
- the panic message is an implementation detail of `String`, so it leaks the
  storage backend through the public API — exactly the coupling ADR-0002 removed.

Week 2 recorded this behavior with a `#[should_panic]` test. That test was a
placeholder for a decision, not a decision.

## Decision

Text operations are fallible. Mutating methods return `TextResult<()>`:

```rust
pub type TextResult<T> = Result<T, TextError>;

pub trait TextStore {
    fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<()>;

    fn delete(&mut self, range: TextRange) -> TextResult<()>;
}
```

`TextError` is a small, explicit enum:

| Variant | Meaning |
|---|---|
| `OffsetOutOfRange { offset, len }` | Offset is past the end of the text |
| `NotCharBoundary { offset }` | Offset is inside a UTF-8 multi-byte character |
| `InvalidRange { start, end }` | Range endpoints are inverted or invalid |

`TextError` implements `Display` and `std::error::Error`, so hosts can format it
or plug it into their own error type. No third-party error crate (thiserror,
anyhow) is used yet: the enum has three variants and the manual implementation is
a dozen lines. See the dependency policy in the development plan.

### Where validation lives

Validation is provided as **default methods on the trait**, not duplicated in
each backend:

```rust
fn is_char_boundary(&self, offset: TextOffset) -> bool;
fn validate_offset(&self, offset: TextOffset) -> TextResult<()>;
fn validate_range(&self, range: TextRange) -> TextResult<()>;
```

Rules:

- Every backend calls `validate_offset` / `validate_range` **before** mutating.
- A rejected operation leaves the text **unchanged**; there are no partial
  edits.
- `is_char_boundary` has a default implementation through `as_str()`, which is
  O(1) for a contiguous backend but may be expensive for a chunked one. Backends
  that can answer cheaper must override it.
- `Document` remains a thin facade: it forwards to the store and performs no
  validation of its own.

This refines ADR-0002, which said validation belonged to "the public API layer".
The correct place is a single shared implementation on the trait: the rule is
part of the offset model, not of a storage backend, and not of `Document` either.

### Breaking change

`insert` and `delete` now return a value. Callers that used them as statements
must handle the result (`?` or `.unwrap()`). This is accepted because the library
is at `0.1.0` and still in the fast-evolution phase described by the API version
policy in the plan.

## Rust concepts considered this week

The week plan also lists trait objects, associated types and `Option`. Their
status:

- **Trait objects**: the trait is object safe, and a test drives a store through
  `&mut dyn TextStore` to verify it. `Document` still holds the concrete
  `StringTextStore` (ADR-0002), because there is no second backend to switch
  between yet.
- **Associated types**: deliberately not introduced. The natural candidate is
  `type Error` on `TextStore`, but a single error taxonomy is simpler while
  there is one backend, and per-backend error types would leak into `Document`.
  Revisit when a second backend exists.
- **`Option`**: no natural use site yet. `TextStore` operations are either valid
  or an error; a "nothing found" case only appears with search (week 18). Adding
  an `Option`-returning API now would be speculative.

## Consequences

- No text operation panics on invalid input; every failure is a `TextError`.
- Failed operations are atomic: the document is unchanged.
- The public API no longer exposes `String`'s panic behavior, so switching to a
  Rope backend cannot change the failure mode.
- Error information is minimal but structured (`offset`, `len`, `start`, `end`),
  which is enough for a host to clamp an offset or highlight a bad range.
- Offset semantics are still UTF-8 byte offsets (ADR-0001). Week 4 re-evaluates
  the position model; `TextError` variants may then need to describe positions in
  a richer way.

## Future

- Grow the error taxonomy only when a real case appears (for example
  `ReadOnly`, `OutOfMemory` or backend-specific failures).
- Reconsider `type Error` when a second backend is added.
- Consider a validation entry point that reports **all** problems in a batch
  edit, once batch edits exist (week 6).
