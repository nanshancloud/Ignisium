# Ignisium

Ignisium is a cross-platform text editor component library written in Rust.

The project aims to provide a modern, extensible and long-term maintainable
text editing core.

## Goals

- Cross-platform
- High performance
- Extensible
- Stable core API
- Long-term API evolution

## Current Status

Early development, weeks 1 to 9 of the development plan are done. The crate
provides the document model and two storage backends:

```text
Document<S> ──► TextStore ──► StringTextStore ──► String
                      └─────► RopeTextStore   ──► Rope (tree of chunks)
```

| Area | State |
|---|---|
| `TextOffset` / `TextRange` | byte offsets, UTF-8 boundary validation (ADR-0001, ADR-0004) |
| `TextStore` | storage abstraction with a range based reading API (ADR-0002, ADR-0008) |
| `TextError` | fallible text operations, no panics on invalid input (ADR-0003) |
| `Document<S>` | storage, metadata and revision, generic over the backend (ADR-0005, ADR-0008) |
| `TextEdit` | atomic edit transactions; every single edit is a one-edit transaction (ADR-0006, ADR-0010) |
| `StringTextStore` | the default backend, one contiguous buffer |
| `RopeTextStore` | rope prototype, a tree of chunks, second backend (ADR-0009) |
| `Rope` | public rope type, every positional method fallible (ADR-0010) |
| Benchmarks | Criterion suite, a scaling probe and a backend comparison probe |
| Storage decision | the rope wins on large documents, the decision is week 10 (ADR-0007) |

Not started yet: line index, selection, undo, search, styling, folding,
layout, rendering and platform abstraction.

## Documentation

| Path | Content |
|---|---|
| `docs/decisions/` | architecture decision records (ADR) |
| `docs/design/` | design documents: text position model, text storage evaluation |
| `docs/benchmark/` | benchmark requirements and published results |
| `docs/dependencies/` | third-party dependency records |

## Development

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

Benchmarks:

```powershell
cargo bench --bench text_store
cargo bench --bench backends
cargo test --release --test scaling -- --ignored --nocapture
cargo test --release --test backend_probe -- --ignored --nocapture
```

## License

See [LICENSE](LICENSE).