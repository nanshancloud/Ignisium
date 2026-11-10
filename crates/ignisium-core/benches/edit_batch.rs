//! Week 11: a large edit batch, measured (G4 of `open-design-gaps.md`).
//!
//! The gate for "validation is a sweep, not a product" is a benchmark, not an
//! argument. This file measures the shape the code had before week 11
//! (`pairwise`, kept here as the reference) and the shape it has now
//! (`Document::edit_problems`, which sorts the claims and sweeps them) on the
//! batch sizes a streaming host produces.
//!
//! Run it with:
//!
//! ```text
//! cargo bench --bench edit_batch
//! ```

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use ignisium_core::{Document, EditBatch, RevisionId, TextEdit, TextOffset, TextRange};

/// Distance between two edits: one replaced byte, three untouched.
const STEP: usize = 4;

/// `count` non-overlapping one-byte edits, spaced over the document - the
/// shape of a set of cursors or of a streamed answer.
fn edits(count: usize) -> Vec<TextEdit> {
    (0..count)
        .map(|index| {
            let start = index * STEP;

            TextEdit::replace(
                TextRange::new(TextOffset(start), TextOffset(start + 1)),
                "x",
            )
        })
        .collect()
}

/// The pre-week-11 shape: every pair of claims compared.
///
/// Kept as a local reference implementation, because the point of the
/// benchmark is the difference between the two forms, and the library no
/// longer contains the slow one.
fn pairwise(claims: &[(usize, usize)]) -> usize {
    let mut conflicts = 0;

    for (index, &(start, end)) in claims.iter().enumerate() {
        for &(other_start, other_end) in claims.iter().take(index) {
            if start < other_end && other_start < end {
                conflicts += 1;
            }
        }
    }

    conflicts
}

/// The claims of `edits`, in the coordinates the pair loop would see.
fn claims(count: usize) -> Vec<(usize, usize)> {
    (0..count)
        .map(|index| (index * STEP, index * STEP + 1))
        .collect()
}

fn bench_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_validation");
    // The pairwise form is quadratic, so the sample has to be small enough to
    // finish: 10 000 edits are 50 million comparisons per iteration.
    group.sample_size(10);

    for count in [1_000usize, 10_000] {
        let document = Document::from_text(&"a".repeat(count * STEP));
        let edits = edits(count);
        let claims = claims(count);

        group.bench_with_input(BenchmarkId::new("pairwise", count), &count, |b, _| {
            b.iter(|| black_box(pairwise(black_box(&claims))));
        });

        group.bench_with_input(BenchmarkId::new("sweep", count), &count, |b, _| {
            b.iter(|| black_box(document.edit_problems(black_box(&edits)).len()));
        });
    }

    group.finish();
}

fn bench_apply(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_apply");
    group.sample_size(10);

    for count in [1_000usize, 10_000] {
        let text = "a".repeat(count * STEP);
        let batch = EditBatch::new(RevisionId::INITIAL, edits(count));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter_batched(
                || Document::from_text(&text),
                |mut document| {
                    let applied = document.apply_batch(&batch).unwrap();

                    black_box(applied.map.map(TextOffset(0)).0);
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

criterion_group!(benches, bench_validation, bench_apply);
criterion_main!(benches);
