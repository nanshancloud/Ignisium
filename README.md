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

Early development, weeks 1 to 10 of the development plan are done. The crate
provides the document model and two storage backends:

```text
Document<S> ──► TextStore ──► StringTextStore ──► String
                      └─────► RopeTextStore   ──► Rope (tree of chunks)
```

| Area | State |
|---|---|
| `TextOffset` / `TextRange` | byte offsets, UTF-8 boundary validation (ADR-0001, ADR-0004) |
| `TextStore` | storage abstraction with a range based reading API, `text_in` and the non-copying `for_each_in` (ADR-0002, ADR-0008, ADR-0011) |
| `TextError` | fallible text operations, no panics on invalid input, `#[non_exhaustive]` (ADR-0003, ADR-0011) |
| `Document<S>` | storage, metadata and revision, generic over the backend (ADR-0005, ADR-0008) |
| `TextEdit` | atomic edit transactions; every single edit is a one-edit transaction (ADR-0006, ADR-0010) |
| `StringTextStore` | the default backend, one contiguous buffer |
| `RopeTextStore` | rope prototype, a tree of chunks, second backend (ADR-0009) |
| `Rope` | public rope type, every positional method fallible (ADR-0010) |
| Benchmarks | Criterion suite, a scaling probe and a backend comparison probe |
| Backend choice | string by default; the rope is opt in for large documents (ADR-0011) |

Not started yet: line index, selection, undo, search, styling, folding,
layout, rendering and platform abstraction.

## Documentation

| Path | Content |
|---|---|
| `docs/decisions/` | architecture decision records (ADR) |
| `docs/design/` | design documents: text position model, text storage evaluation, open design gaps |
| `docs/benchmark/` | benchmark requirements and published results |
| `docs/dependencies/` | third-party dependency records |

`docs/design/open-design-gaps.md` lists the questions the design does not
answer yet, each with the week that has to answer it, so that a later module
does not rediscover them as a defect.

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

# The probes run one at a time: they share a global allocation counter.
cargo test --release --test backend_probe backend_comparison -- --ignored --nocapture
cargo test --release --test backend_probe memory_comparison -- --ignored --nocapture
cargo test --release --test backend_probe read_strategy_comparison -- --ignored --nocapture
```

## License

See [LICENSE](LICENSE).