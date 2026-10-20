# Dependency: criterion

First third-party dependency of the project. Recorded per the dependency
governance rules in the development plan (section 7).

| Item | Value |
|---|---|
| Dependency | `criterion` 0.8 (dev-dependency only) |
| Purpose | micro benchmarks for the storage layer (week 7) |
| Public API exposure | none: it is a development dependency and never appears in `ignisium-core`'s public API |
| Replaceability | high: benchmarks call the public API only, so the tool can be swapped without touching the crate |
| MSRV | compatible with the toolchain in use (rustc 1.99) |
| Platform impact | none: the crate builds on every platform, the benchmark harness does not affect the library build |
| Exit plan | remove the dependency and the `benches/` target, or replace the harness; `docs/benchmark/` keeps the requirements and the published numbers |

## Why a dependency at all

The plan allows third-party crates as long as they sit behind an internal
boundary, are recorded here, and never appear in the stable public API. A
benchmark harness is the smallest possible case of that: it is a development
tool, it touches no production code path, and removing it changes nothing
about the library.

## What the project still does itself

The scaling probe (`crates/ignisium-core/tests/scaling.rs`) is written without
any dependency, on purpose:

- it has to measure 100 MiB documents, which is outside what a statistical
  micro benchmark harness is good at;
- it prints a table instead of a statistical report, which is the form the
  week 7 and week 8 documents quote;
- it keeps the "how do we measure" question answerable without a network
  connection and without a lock file.

## Rules for future dependencies

1. A dependency has an entry in `docs/dependencies/` before it is used.
2. A dependency never appears in the public API of `ignisium-core`.
3. A dependency has a written exit plan.
4. The core keeps working when the dependency is removed: the "delete
   exercise" from the plan (section 22.9).