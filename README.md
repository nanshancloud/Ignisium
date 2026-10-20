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

Early development, weeks 1 to 8 of the development plan are done. The crate
currently provides the document model:

```text
Document ──► TextStore ──► StringTextStore ──► String
```

| Area | State |
|---|---|
| `TextOffset` / `TextRange` | byte offsets, UTF-8 boundary validation (ADR-0001, ADR-0004) |
| `TextStore` | storage abstraction with a `String` backend (ADR-0002) |
| `TextError` | fallible text operations, no panics on invalid input (ADR-0003) |
| `Document` | storage, metadata and revision (ADR-0005) |
| `TextEdit` | atomic edit transactions, batch validation (ADR-0006) |
| Benchmarks | Criterion suite plus a 1 MiB / 10 MiB / 100 MiB scaling probe |
| Storage decision | rope prototype is next, `TextStore` is reshaped first (ADR-0007) |

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
cargo test --release --test scaling -- --ignored --nocapture
```

## License

See [LICENSE](LICENSE).