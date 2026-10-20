# Week 7 Baseline: StringTextStore

Measured baseline for the only storage backend that exists today. It is the
reference point for week 9 (rope prototype) and week 10 (backend comparison).

## Environment

| Item | Value |
|---|---|
| Machine | Windows x64, consumer CPU |
| Toolchain | rustc 1.99.0, cargo 1.99.0 |
| Profile | `release` (the numbers below are from `--release`) |
| Criterion | 0.8, `--warm-up-time 1 --measurement-time 2` |
| Backend | `StringTextStore` (one contiguous `String`) |
| Input | ASCII (`"a".repeat(n)`), and `"中".repeat(n)` for the Unicode group |

Reproduce:

```text
cargo bench --bench text_store
cargo test --release --test scaling -- --ignored --nocapture
```

## Criterion results: one edit on a prepared document

Mean time per operation, document already built (setup is not measured).

| Group | Size | Mean time |
|---|---:|---:|
| `append` | 1 KiB | 186 ns |
| `append` | 64 KiB | 4.47 us |
| `append` | 1 MiB | 330 us |
| `middle_insert` | 1 KiB | 108 ns |
| `middle_insert` | 64 KiB | 4.07 us |
| `middle_insert` | 1 MiB | 346 us |
| `delete` (10% of the document) | 1 KiB | 67 ns |
| `delete` (10% of the document) | 64 KiB | 1.95 us |
| `delete` (10% of the document) | 1 MiB | 76.2 us |
| `unicode` (insert one CJK char) | 1 Ki chars (3 KiB) | 185 ns |
| `unicode` (insert one CJK char) | 64 Ki chars (192 KiB) | 12.8 us |
| `large_document` append | 10 MiB | 2.66 ms |
| `large_document` middle_insert | 10 MiB | 3.13 ms |

## Scaling probe: 1 MiB, 10 MiB, 100 MiB

`cargo test --release --test scaling -- --ignored --nocapture`

| Size | Operation | Time | Throughput |
|---:|---|---:|---:|
| 1 MiB | load | 332 us | 3007 MiB/s |
| 1 MiB | append 1 byte | 304 us | 3287 MiB/s |
| 1 MiB | middle insert 1 byte | 364 us | 2744 MiB/s |
| 1 MiB | delete 10% | 17.8 us | 5618 MiB/s |
| 10 MiB | load | 4.01 ms | 2494 MiB/s |
| 10 MiB | append 1 byte | 3.41 ms | 2930 MiB/s |
| 10 MiB | middle insert 1 byte | 4.85 ms | 2062 MiB/s |
| 10 MiB | delete 10% | 691 us | 1447 MiB/s |
| 100 MiB | load | 42.4 ms | 2358 MiB/s |
| 100 MiB | append 1 byte | 39.1 ms | 2558 MiB/s |
| 100 MiB | middle insert 1 byte | 43.7 ms | 2288 MiB/s |
| 100 MiB | delete 10% | 4.62 ms | 2164 MiB/s |

## What the numbers say

**1. Appending one byte to a 100 MiB document costs 39 ms.**
The document was loaded with `Document::from_text`, which leaves the `String`
with a capacity equal to its length. The first byte that does not fit forces a
reallocation, and the reallocation copies the whole 100 MiB. The cost of
"appending" is therefore the cost of "copying the document", not the cost of
the append. A tree based backend would append one chunk.

**2. A middle insert costs about the same as a full copy.**
Inserting one byte in the middle of a 100 MiB document moves the whole tail
(43.7 ms). This is the expected behaviour of a contiguous buffer and the
reason a rope is worth prototyping.

**3. The cost of a delete is proportional to the text that follows it.**
Deleting 10% of a 100 MiB document takes 4.6 ms, because the remaining 90%
has to be moved. The throughput stays around 2 GiB/s, so the constant factor
is the memory bandwidth of the machine.

**4. Small documents are not a problem.**
Every operation on documents up to 64 KiB is in the microsecond range. The
string backend is the right choice for them, and it should stay the default
for them.

**5. Unicode costs bytes, not characters.**
A 64 Ki character CJK document is 192 KiB of bytes, and inserting one
character into it costs 12.8 us, in line with the ASCII numbers for a
comparable byte count. The storage layer has no special Unicode penalty,
because it counts bytes (ADR-0001).

## Consequences

- The string backend is acceptable up to roughly a megabyte and clearly
  insufficient beyond ten megabytes.
- Week 9 has a concrete target to beat: **an append and a middle insert must
  not cost O(document size)**.
- The "one byte append" result is a *reallocation* artifact, not a property of
  the data structure. It can be mitigated cheaply (reserve capacity when
  loading) and the week 9 prototype must be compared with the same mitigation
  applied, otherwise the comparison would be unfair.