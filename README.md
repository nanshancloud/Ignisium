# Ignisium

A cross-platform text editor core, written in Rust.

Ignisium is the text engine a host application embeds: it owns the text, the
edits and the positions, and it leaves the window, the input and the platform to
the host. It aims at a small stable API that can evolve for years.

## Status

Early development. Weeks 1 to 11 of the development plan are done: a document
model, two storage backends and an edit pipeline.

```text
Document<S> ──► TextStore ──► StringTextStore ──► String
                      └─────► RopeTextStore   ──► Rope (tree of chunks)
```

| Works today | |
|---|---|
| Positions | `TextOffset` / `TextRange`, UTF-8 byte offsets, invalid input is an error |
| Storage | `TextStore` abstraction: a whole text, a range, or a range piece by piece |
| Documents | `Document<S>` with content, metadata and a monotonic revision |
| Edits | one edit or a batch, atomic, one revision per batch |
| Remote edits | `EditBatch` names its revision; a stale batch is rejected |
| Offset map | `AppliedEdits` tells a host where a cursor or a range moved |
| Backends | `StringTextStore` (default) and `RopeTextStore` (opt-in, experimental) |
| Tests | one conformance suite runs against every backend |

Missing: line index, selection, undo, search, styling, layout, rendering and the
platform layer.

## Example

```rust
use ignisium_core::{Document, EditBatch, TextEdit, TextOffset, TextRange};

let mut document = Document::from_text("Hello world");

document
    .apply_edits(&[TextEdit::replace(TextRange::new(TextOffset(6), TextOffset(11)), "there")])
    .unwrap();
assert_eq!(document.text(), "Hello there");

// An edit that comes from outside names the revision it was written against,
// and the document answers where the old offsets point now.
let at = document.revision();
let batch = EditBatch::new(at, vec![TextEdit::insert(TextOffset(0), ">> ")]);
let applied = document.apply_batch(&batch).unwrap();

assert_eq!(document.text(), ">> Hello there");
assert_eq!(applied.map.map(TextOffset(6)), TextOffset(9));
```

## Design rules

- **Byte offsets are the only coordinate.** No line/column, no chunk index, no
  unit conversion at the boundary.
- **Host input never panics.** Invalid positions are `TextError` values; only a
  broken library invariant is an assertion.
- **One entry point per capability.** A single edit is a one-edit transaction, so
  there is one set of rules, not two.
- **The storage layout stays invisible.** Reading happens through ranges and a
  visitor; a chunk never reaches the host.

Details and the reasoning are in [`docs/decisions/`](docs/decisions) (ADR-0001 to
ADR-0014).

## Documentation

| Path | Content |
|---|---|
| [`docs/decisions/`](docs/decisions) | architecture decision records |
| [`docs/design/`](docs/design) | position model, storage evaluation, open design gaps |
| [`docs/benchmark/`](docs/benchmark) | measured results and the requirements behind them |

## Development

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace          # includes the cross-backend conformance suite
```

Benchmarks:

```powershell
cargo bench --bench text_store  # operations on both backends
cargo bench --bench backends    # string vs rope, one document at a time
cargo bench --bench edit_batch  # batch validation and apply, 1k and 10k edits
```

The probes in `tests/backend_probe.rs` and `tests/scaling.rs` are `#[ignore]`d
because they allocate hundreds of megabytes and share a global allocation
counter; run them one at a time:

```powershell
cargo test --release --test scaling -- --ignored --nocapture
cargo test --release --test backend_probe backend_comparison -- --ignored --nocapture
cargo test --release --test backend_probe memory_comparison -- --ignored --nocapture
cargo test --release --test backend_probe read_strategy_comparison -- --ignored --nocapture
```

## License

See [LICENSE](LICENSE).
