# Week 10: Rope vs String

The decision record is ADR-0011. This document is the data behind it.

## Environment

| Item | Value |
|---|---|
| Machine | Windows x64, consumer CPU |
| Profile | `release` |
| Backends | `StringTextStore` (contiguous) and `RopeTextStore` (B+ tree of 1 KiB chunks) |
| Input | ASCII, no line breaks |
| Statistics | median of 21 runs; document built before the timer |

Reproduce (one probe at a time, the allocation counter is global):

```text
cargo test --release --test backend_probe backend_comparison -- --ignored --nocapture
cargo test --release --test backend_probe memory_comparison -- --ignored --nocapture
cargo test --release --test backend_probe read_strategy_comparison -- --ignored --nocapture
```

`ratio` is `string / rope`: above 1 means the rope is faster.

## Edits

| Size | Operation | String | Rope | Ratio |
|---:|---|---:|---:|---:|
| 1 KiB | append | 200 ns | 1100 ns | 0.18x |
| 1 KiB | middle insert | 200 ns | 1100 ns | 0.18x |
| 64 KiB | append | 200 ns | 900 ns | 0.22x |
| 64 KiB | middle insert | 1500 ns | 900 ns | 1.67x |
| 1 MiB | append | 191.0 us | 1.8 us | **106x** |
| 1 MiB | middle insert | 216.2 us | 1.0 us | **216x** |
| 10 MiB | append | 1926.9 us | 2.1 us | **918x** |
| 10 MiB | middle insert | 2336.8 us | 2.4 us | **974x** |
| 100 MiB | append | 21530.2 us | 2.4 us | **8971x** |
| 100 MiB | middle insert | 24379.4 us | 4.2 us | **5805x** |
| 1 KiB | delete 10% | 100 ns | 200 ns | 0.50x |
| 1 MiB | delete 10% | 18.2 us | 12.9 us | 1.41x |
| 10 MiB | delete 10% | 429.4 us | 118.3 us | 3.63x |
| 100 MiB | delete 10% | 3921.8 us | 1241.4 us | 3.16x |

The rope is flat in the document size: an append costs 2.4 us at 100 MiB, the
same as at 1 MiB. The string backend grows linearly, and at 100 MiB a single
byte append costs 21.5 ms - more than a frame at 60 Hz.

**Crossover.** The rope loses below about 64 KiB for appends and between 64 KiB
and 1 MiB for middle inserts. Below that the tree descent costs more than
moving the buffer.

## Reads

`text_in` copies the range on a chunked backend, so it is not the only read
that matters. `for_each_in` visits the chunks instead of copying them.

| Size | Read | Time | Copied |
|---:|---|---:|---:|
| 1 MiB | string `text_in` | 100 ns | 0 B |
| 1 MiB | rope `text_in` | 56.2 us | 102.4 KiB |
| 1 MiB | rope `for_each_in` | 700 ns | 0 B |
| 10 MiB | string `text_in` | 300 ns | 0 B |
| 10 MiB | rope `text_in` | 547.6 us | 1.0 MiB |
| 10 MiB | rope `for_each_in` | 6.9 us | 0 B |
| 100 MiB | string `text_in` | 500 ns | 0 B |
| 100 MiB | rope `text_in` | 5525.3 us | 10.0 MiB |
| 100 MiB | rope `for_each_in` | 144.3 us | 0 B |

`for_each_in` is 79x faster than `text_in` on the rope at 10 MiB and 38x at
100 MiB, and it allocates nothing. It is still 23x slower than the string
backend handing out a borrowed slice, because it calls the closure once per
chunk (1024 times for 1 MiB of range). That is the residual cost of not having
the text in one buffer, and it is small enough for a viewport: 6.9 us for a
1 MiB range.

## Memory

Retained after construction, and the peak while applying one middle insert.

| Size | String | Rope | Rope/String | String peak | Rope peak | Peak ratio |
|---:|---:|---:|---:|---:|---:|---:|
| 1 KiB | 1.0 KiB | 1.0 KiB | 1.00x | 2.1 KiB | 3.3 KiB | 1.61x |
| 64 KiB | 64.0 KiB | 68.0 KiB | 1.06x | 128.1 KiB | 69.2 KiB | 0.54x |
| 1 MiB | 1.0 MiB | 1.1 MiB | 1.06x | 2.0 MiB | 1.1 MiB | 0.54x |
| 10 MiB | 10.0 MiB | 10.6 MiB | 1.06x | 20.0 MiB | 10.8 MiB | 0.54x |
| 100 MiB | 100.0 MiB | 106.3 MiB | 1.06x | 200.0 MiB | 107.8 MiB | 0.54x |

Two results, both better than expected:

1. **The rope costs 6% more memory, not 100%.** A 1 KiB chunk carries a `String`
   header and an internal node per eight chunks; at this chunk size the
   overhead is small and predictable.
2. **An edit needs half the peak memory.** The string backend's `replace_range`
   makes the buffer grow to twice the text, so editing a 100 MiB document peaks
   at 200 MiB. The rope copies one chunk and peaks at 108 MiB.

The 1 KiB row is the exception: with a single chunk the rope pays its fixed
overhead (tree nodes, vector) on almost no text.

## Snapshots

| Size | String | Rope | Ratio |
|---:|---:|---:|---:|
| 1 MiB | 254.3 us | 242.4 us | 1.05x |
| 10 MiB | 1983.4 us | 2506.0 us | 0.79x |
| 100 MiB | 21484.8 us | 26458.8 us | 0.81x |

Neither backend can snapshot cheaply, because the rope prototype copies its
nodes instead of sharing them (ADR-0009). The rope is 20% slower here: the copy
of the tree is work the string backend does not do. Sharing the nodes would
make this O(1), and it is the one place where the prototype is behind a
production rope.

## What this settles

| Question | Answer |
|---|---|
| Which backend by default | string; the rope is opt-in for large documents |
| Where is the crossover | about 64 KiB for appends, 64 KiB to 1 MiB for middle inserts |
| Is the rope's memory a deal breaker | no: 6% retained, 46% less peak per edit |
| How to read a range on a chunked backend | `for_each_in`, not `text_in` |
| Is the rope ready to be the default for large files | yes for editing, no for snapshots |

## What is still not measured

- Long single lines (week 12).
- Many small documents open at once (the fixed per-document overhead).
- The rope's behaviour under repeated deletion: ADR-0010 collapses a node left
  with one child, but does not merge underfull siblings (ADR-0009).