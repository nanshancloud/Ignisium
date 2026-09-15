# ADR-0002: TextStore Abstraction

## Status

Accepted.

## Decision

`Document` should not directly depend on `String` as its conceptual text storage
abstraction.

The text storage layer is represented by the `TextStore` trait:

```rust
pub trait TextStore {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool;

    fn as_str(&self) -> &str;

    fn insert(&mut self, offset: TextOffset, text: &str);

    fn delete(&mut self, range: TextRange);
}
```

The initial implementation uses `StringTextStore`, which is backed by a
contiguous `String`.

The `text` module is private and re-exports its public items through
`pub use`, so the internal module layout is not part of the public API:

```rust
use ignisium_core::text::TextStore;
```

## Reason

The public API should not permanently depend on the internal text storage
implementation. Introducing the abstraction now, while the code base is small,
keeps the boundary cheap to change and lets the editor core be tested against a
simple backend.

## Scope

The first version keeps the coupling concrete on purpose:

```rust
pub struct Document {
    store: StringTextStore,
}
```

Generic documents (`Document<S: TextStore>`) and dynamic dispatch
(`Box<dyn TextStore>`) are deliberately postponed. The trade-offs are not
understood well enough yet, and adding either now would spread generics or
allocation decisions through the API before there is a second backend to
validate them against.

## Future implementations may use

- Rope
- Piece Table
- Gap Buffer
- Chunked storage

## Consequences

- `Document` only performs text mutations through `TextStore`.
- Offsets remain UTF-8 byte offsets (see ADR-0001), so a future backend must
  keep the same offset semantics or provide an explicit conversion layer.
- An offset inside a multi-byte character currently panics. Offset validation
  belongs in the public API layer, not in each backend.
