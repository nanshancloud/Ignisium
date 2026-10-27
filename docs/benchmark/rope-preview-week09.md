# Week 9: Rope Preview

Measured comparison of the two storage backends. This is the data the week 10
decision is based on; the decision itself is still open (ADR-0007).

## Environment

| Item | Value |
|---|---|
| Machine | Windows x64, consumer CPU |
| Toolchain | rustc 1.99.0 |
| Profile | `release` |
| Backends | `StringTextStore` (contiguous) and `RopeTextStore` (rope prototype) |
| Input | ASCII, one chunk of 1 KiB, no line breaks |

Reproduce:

```text
cargo test --release --test backend_probe -- --ignored --nocapture
```

## Method

The Criterion benchmarks (`benches/backends.rs`) turned out to be unsuitable
for this comparison: the document has to be rebuilt for every iteration, and
the rebuild of the input is part of the measured time. For a contiguous
backend that rebuild is a copy of the whole text, for the rope it is a copy of
the whole tree - a term that has nothing to do with the operation under test.
At 1 MiB the copy is worth about 35 us, which is more than the whole edit of
the rope.

The probe therefore builds the document **before** the timer starts and times
only the operation. Every number is the median of 21 runs. The clock of this
platform has a resolution of about 100 ns, so operations below that are
reported as 0 ns or 100 ns.

## Results

`ratio` is `string / rope`: above 1 means the rope is faster.

### Edits

| Size | Operation | String | Rope | Ratio |
|---:|---|---:|---:|---:|
| 1 KiB | append | 100 ns | 900 ns | 0.11x |
| 1 KiB | middle insert | 100 ns | 800 ns | 0.12x |
| 64 KiB | append | 100 ns | 500 ns | 0.20x |
| 64 KiB | middle insert | 400 ns | 300 ns | 1.33x |
| 1 MiB | append | 188.6 us | 1.5 us | **125.7x** |
| 1 MiB | middle insert | 216.9 us | 0.9 us | **241.0x** |
| 1 MiB | delete 10% | 13.7 us | 9.2 us | 1.49x |
| 10 MiB | append | 2067.5 us | 1.5 us | **1378.3x** |
| 10 MiB | middle insert | 2352.3 us | 2.0 us | **1176.2x** |
| 10 MiB | delete 10% | 413.7 us | 112.6 us | 3.67x |

### Reads

| Size | Operation | String | Rope | Ratio |
|---:|---|---:|---:|---:|
| 1 KiB | read 10% | 0 ns | 100 ns | - |
| 64 KiB | read 10% | 0 ns | 2.5 us | - |
| 1 MiB | read 10% | 100 ns | 44.6 us | - |
| 10 MiB | read 10% | 200 ns | 579.2 us | - |
| 1 KiB | snapshot | 100 ns | 100 ns | 1.00x |
| 64 KiB | snapshot | 17.0 us | 25.5 us | 0.67x |
| 1 MiB | snapshot | 234.9 us | 251.6 us | 0.93x |
| 10 MiB | snapshot | 2095.4 us | 2582.3 us | 0.81x |

## What the numbers say

**1. The rope removes the O(n) edit cost.** An append or a middle insert on a
10 MiB document costs 1.5 us instead of 2067 us. The string backend copies the
whole buffer; the rope copies one chunk. This is the effect ADR-0007 predicted,
and it is the reason the rope is worth building.

**2. The rope is slower on small documents.** At 1 KiB it is 8 to 9 times
slower than the contiguous buffer: the tree, the descent and the chunk
handling cost more than moving a kilobyte. The string backend therefore stays
the default for small documents. The crossover in this measurement is between
64 KiB and 1 MiB.

**3. The range read is a regression.** Reading a tenth of a 10 MiB document
costs 579 us with the rope and 200 ns with the string backend. The string
backend hands out a borrowed slice; the rope has to copy the range out of its
chunks. This is the price of the decision in ADR-0008 that the trait returns
text and never chunks. It is the one scenario where the rope is clearly
worse, and it matters: a viewport, a search window and the line index of week
13 all read ranges.

Two ways out, both for week 10:

- a range read that visits the chunks instead of copying them, so that the
  host can work on the pieces (this changes the trait again);
- a rope whose chunks are shared, so that a range read can borrow the chunk
  that holds it.

**4. The snapshot does not win yet.** The rope copies the text for a snapshot
too, because the prototype does not share its nodes (ADR-0009). The number
shows the cost of the tree bookkeeping on top of the copy: 2582 us against
2095 us at 10 MiB, so about 20% overhead. Sharing the nodes would make the
snapshot O(1) instead of O(n) - that is the single most valuable change for
the production version, because the background work of week 18 needs it.

**5. Deleting is faster with the rope, but not by much.** At 10 MiB, 112 us
against 413 us. The delete still has to move the text that follows the deleted
range, and the rope moves it chunk by chunk.

## What was not measured

- **Memory.** The benchmark requirements list the peak memory of an operation
  as a metric, and the memory overhead of the rope is the price it pays for
  the speed above. It is not measured yet. This is the most important missing
  number for week 10: a rope that needs twice the memory of the text is a
  different deal than one that needs 1.3 times.
- **A 100 MiB document.** The week 7 baseline already showed that the string
  backend needs 39 ms for a one byte append at 100 MiB. The rope is expected
  to be flat there, but the number has to be measured.
- **Long single lines** (week 12) and **many small documents**.

## Consequences for week 10

1. The question is not "rope or string", it is "which backend for which
   document size". The answer is at least: string below about 64 KiB, rope
   above about 1 MiB, and a decision in between.
2. The range read has to be part of the decision. The rope is much better at
   editing and much worse at reading ranges, and a text editor does both.
3. The memory overhead has to be measured before the rope can be the default
   for large documents.
4. The public API question of ADR-0008 (generic parameter, `Box<dyn
   TextStore>` or an enum) can now be answered with two real backends instead
   of one, but the answer should also say how a host picks a backend.