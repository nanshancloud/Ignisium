# ADR-0008: Range Based TextStore and Generic Document

## Status

Accepted.

Implements decision 3 of ADR-0007 ("`TextStore` has to be reshaped before the
rope"). Breaking change, accepted at `0.1.0` per the API version policy.

## Context

ADR-0002 introduced `TextStore` so the storage backend could be replaced, and
kept `Document` bound to the concrete `StringTextStore`. The trait was written
for a contiguous backend:

```rust
pub trait TextStore {
    fn as_str(&self) -> &str;   // one contiguous slice
    ...
}
```

ADR-0007 compared the candidates and found that the rope, the piece table and
the gap buffer all store the text in pieces. None of them can return a `&str`
for the whole document without flattening it, which means copying the whole
document or keeping a permanently stale copy. The trait was the reason a
second backend could not exist, and it had to be reshaped before the rope
prototype of week 9 could be written at all.

Two related questions come with the reshape.

**How do hosts read a part of the text?** The trait had no way to ask for a
range, because a contiguous backend can slice and a chunked backend can only
answer "all of it". But the whole point of a chunked backend is that a host
should not have to materialise the whole document to look at a line, a screen
full of text or a search window. The range read is the operation that makes a
chunked backend useful.

**How does `Document` stay the same for every backend?** With one backend,
`Document` could hold it concretely and return `&str` from `text()`. With two
backends, `Document` has to be parameterised over the store, and `text()` has
to return something that works for both: a contiguous backend can borrow, a
chunked backend has to own. `Cow<str>` is the type that says "borrowed if
possible".

## Decision

### 1. The trait reads text and ranges, never chunks

```rust
pub trait TextStore {
    fn len(&self) -> usize;

    /// The whole text.
    fn text(&self) -> Cow<'_, str>;

    /// A range of the text, `[start, end)`.
    fn text_in(&self, range: TextRange) -> TextResult<Cow<'_, str>>;

    fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<()>;
    fn delete(&mut self, range: TextRange) -> TextResult<()>;

    fn is_char_boundary(&self, offset: TextOffset) -> bool;
}
```

Rules:

- **The trait returns text, never storage.** A host that asks for a range gets
  a string, not a list of chunks. The chunking stays behind the boundary, so
  the storage layout can change without changing the API (R8 of the week 8
  evaluation).
- **`text()` and `text_in()` return snapshots.** A contiguous backend answers
  with `Cow::Borrowed`, a chunked backend with `Cow::Owned`. Neither is a view
  that stays valid after the next edit.
- **`text_in()` is fallible.** The range is validated first, so an invalid
  range is a `TextError` and never a panic. This is the same rule the
  mutating operations follow (ADR-0003).
- **`is_char_boundary()` is now a required method.** It used to have a default
  implementation that went through `as_str()`. For a chunked backend that
  default would materialise the whole document for a check that every edit
  performs, so each backend has to answer it on its own.
- **`as_str()` leaves the trait.** It stays an inherent method of
  `StringTextStore`, where borrowing is free and honest. A host that knows it
  has a contiguous backend can still use it.

### 2. `Document` becomes generic over its store

```rust
pub struct Document<S: TextStore = StringTextStore> {
    store: S,
    metadata: DocumentMetadata,
    revision: RevisionId,
}
```

The default type parameter keeps the existing spelling valid:
`Document`, `Document::new()`, `Document::from_text()` and `let d: Document`
all still refer to the string backend. A second backend is used explicitly:

```rust
let document = Document::from_store(RopeTextStore::new());
```

The generic parameter is the *mechanism* that allows several backends. The
public API question - generic parameter, `Box<dyn TextStore>`, or an enum of
backends - is still open and is decided in week 10 with measurements, as
ADR-0007 requires. A generic parameter is the cheapest way to get there
without deciding anything else yet.

### 3. `Document::text()` returns `Cow<str>`

```rust
pub fn text(&self) -> Cow<'_, str>;
pub fn text_in(&self, range: TextRange) -> TextResult<Cow<'_, str>>;
```

This is the visible consequence of the reshape: the document can no longer
promise a borrowed whole text, because it does not know how the text is
stored. For the string backend the result is still borrowed, so the common
case does not allocate.

### 4. The store is readable, but not writable from outside

```rust
pub fn store(&self) -> &S;
```

There is no `store_mut()`. A host that could change the text through the
store would break the invariant that the revision describes the content
(ADR-0005). Read-only access is enough for the measurements of week 10 and
for backend specific queries such as the chunk count of a rope.

### 5. A latent panic is fixed on the way

`StringTextStore::is_char_boundary` used `str::is_char_boundary` directly,
which panics for an index past the end of the string. The method is public, so
a host that passed an offset from outside the document could take the process
down. It now returns `false` for such an offset, which is the same answer the
rest of the API gives for an invalid position.

## Consequences

- A second backend becomes possible: the trait no longer demands a contiguous
  buffer.
- `Document::text()` returns `Cow<str>` instead of `&str`. Every host that
  compared the result with a string literal keeps working (`Cow<str>` compares
  with `&str`), but a host that stored the borrow now has to own the value.
- The document can be built on any backend, and the document level rules
  (revision, metadata, transactions) are backend independent by construction:
  the transaction tests of week 6 now run against every backend.
- The trait is slightly larger and slightly more expensive for a contiguous
  backend: `text()` returns a `Cow` instead of a `&str`. The cost is one
  enum discriminant, and the benefit is that the boundary describes what the
  core needs instead of what the first implementation happened to provide.
- The storage layout is invisible to hosts. A rope can change its chunk size
  without a single change in the public API.

## Breaking changes

| Change | Migration |
|---|---|
| `TextStore::as_str` removed from the trait | use `TextStore::text`, or `StringTextStore::as_str` for the contiguous backend |
| `TextStore::text` and `TextStore::text_in` added | implement them in a custom backend |
| `TextStore::is_char_boundary` has no default | implement it in a custom backend |
| `Document::text` returns `Cow<str>` | `.text()` still compares with `&str`; `&document.text()` no longer borrows the document |
| `Document` is generic | `Document`, `Document::new()` and `Document::from_text()` are unchanged for the default backend |

## Lifecycle

| Item | Lifecycle | Notes |
|---|---|---|
| `TextStore::text` / `text_in` | Stable | the reading contract of every backend |
| `Document<S>` | Stable | generic over the backend |
| `StringTextStore::as_str` | Stable | inherent, contiguous backend only |
| `Document::store` | Experimental | exists for measurements; may be replaced by capability queries |

## Future

- Week 9 implements the rope behind this trait (ADR-0009).
- Week 10 measures both backends and decides how the public API should offer
  the choice: generic parameter, `Box<dyn TextStore>` or an explicit enum.
- A backend that can answer "how many chunks" or "how much memory" gets a
  capability query instead of exposing its types in the public API.