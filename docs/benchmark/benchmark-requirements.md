# Benchmark Requirements

Week 8 deliverable: what Ignisium has to measure, how, and what the numbers
have to mean before a storage decision is made. Companion to
`docs/design/text-storage-evaluation.md` and ADR-0007.

The benchmarks of week 7 exist (`crates/ignisium-core/benches/text_store.rs`
and `crates/ignisium-core/tests/scaling.rs`). This document states the
requirements the benchmark suite has to keep satisfying, so that week 9 and
week 10 measure the same thing in the same way.

---

## 1. Why this document exists

Two backends can only be compared if they are measured with the same
questions, on the same inputs, in the same build profile. Without that, the
comparison becomes an argument about numbers collected under different
conditions. The rules below are the contract for every future benchmark.

## 2. Scenarios

The scenarios are named after the situations the core has to serve, not after
the API calls that implement them.

| # | Scenario | What it represents | Planned week |
|---|---|---|---|
| S1 | `append` | typing at the end of the document | W7 (done) |
| S2 | `middle_insert` | typing in the middle, the worst case for a contiguous buffer | W7 (done) |
| S3 | `delete` | removing a range | W7 (done) |
| S4 | `unicode_edit` | the same operations on non-ASCII text | W7 (done) |
| S5 | `load` | opening a document of a given size | W7 (done) |
| S6 | `snapshot` | copying the document for a background task | W9 |
| S7 | `transaction` | applying a batch of edits (ADR-0006) | W9 |
| S8 | `read_range` | reading a window of the document for layout or search | W9 |
| S9 | `line_lookup` | offset to line and line to offset | W13 |
| S10 | `long_line` | a 10 MB, 100 MB and 500 MB single line | W12 |
| S11 | `search` | scanning the document | W18 |
| S12 | `incremental_styling` | restyling only the changed region | W20 |

## 3. Document sizes

| Name | Bytes | Typical use |
|---|---:|---|
| `tiny` | 1 KiB | a script, a config file |
| `small` | 64 KiB | a source file |
| `medium` | 1 MiB | a large source file, a generated file |
| `large` | 10 MiB | a big log, a dump |
| `huge` | 100 MiB | the large file scenario of the plan |
| `extreme` | 1 GiB | only where the machine allows it |

Sizes are given in **bytes**, because the storage layer counts bytes
(ADR-0001). A Unicode document is described by its byte size as well, so that
a 3-byte-per-character text is compared against ASCII of the same byte count,
not the same character count.

## 4. Rules

1. **One operation per measurement.** A benchmark measures a single edit, a
   single read or a single lookup on a prepared document.
2. **Preparation is not measured.** Building the document is setup. For the
   Criterion benchmarks this is a trap: `iter_batched` with
   `BatchSize::SmallInput` rebuilds the input for every iteration, and that
   rebuild is part of the measured time. A contiguous backend pays a copy of
   the whole text, a rope pays a copy of the whole tree. Where the two
   backends are compared, the measurement uses a probe that builds the
   document before the timer starts (`tests/backend_probe.rs`).
3. **Release profile only.** Debug numbers are not comparable with release
   numbers and are not published.
4. **Same machine, same session.** Numbers from different machines are not
   compared. Every published table names the machine.
5. **The input is stated.** Content (ASCII, CJK, mixed), line structure (one
   line, many lines) and size are part of the result.
6. **Throughput is reported next to the time.** A number in nanoseconds says
   little without knowing how many bytes it moved.
7. **The baseline is kept.** The week 7 numbers are the reference for every
   later comparison, and a change that makes an operation slower has to be
   argued for explicitly.
8. **Worst case first.** For every scenario, the position that is expensive
   for the current backend (middle insert) is measured, not only the cheap
   one (append).

## 5. Metrics

| Metric | Meaning | Used for |
|---|---|---|
| mean time | average time per operation | the default comparison |
| median time | typical time, robust against outliers | noisy environments |
| worst case | slowest observed run | latency budgets for a host |
| throughput | bytes moved per second | comparing backends on large inputs |
| peak memory | additional bytes allocated per operation | the memory cost of a snapshot or a chunk copy |

Peak memory is not measured by Criterion. It has to come from a separate
probe, because it is the number that decides whether a tree based backend is
acceptable: a rope trades time for memory overhead, and that trade has to be
visible in the report, not only in the intuition.

## 6. Acceptance

A backend may replace `StringTextStore` as the default only if it satisfies
all of the following on the `medium`, `large` and `huge` sizes:

| Requirement | Threshold |
|---|---|
| S2 middle insert | not more than 10x the time of a small document |
| S1 append | not more than 10x the time of a small document |
| S6 snapshot | at most 1.1x the document size in additional memory |
| S9 line lookup | O(log n) in the number of lines |
| Unicode | no penalty beyond the byte count |
| Every scenario | no regression against the week 7 baseline for documents up to `small` |

The "10x" thresholds encode the actual requirement: an operation must not
cost more than a constant factor of the document size. A backend that copies
the whole document for a one byte edit fails this, no matter how fast the
machine is.

## 7. Reporting

Every benchmark run that leads to a decision is recorded as a document in
`docs/benchmark/`:

```text
docs/benchmark/
├── benchmark-requirements.md   this document
├── baseline-week07.md          the string backend baseline
├── rope-preview-week09.md      the first comparison of the two backends
├── rope-week10.md              the decision (planned)
└── long-line-week12.md         the single line scenario (planned)
```

A report names the machine, the toolchain, the commit it was produced from
and the scenario it covers, so that a number can be traced back to the code
that produced it.

## 8. Out of scope

- **Comparison with other editors.** Not reproducible, not a core question.
- **User level latency.** The core does not render; a host's frame time is
  not a core metric.
- **Micro-optimising the current backend before the data demands it.** The
  week 7 baseline is good enough for small documents; there is nothing to
  gain from tuning `String` before the rope prototype exists.