//! Week 9: the two storage backends, measured against each other.
//!
//! This is the preview of the week 10 comparison. Every operation is measured
//! on both backends with the same input, in the same build, on the same
//! machine, so that the numbers can be compared directly.
//!
//! Run it with:
//!
//! ```text
//! cargo bench --bench backends
//! ```
//!
//! The rules of `docs/benchmark/benchmark-requirements.md` apply: one
//! operation per measurement, preparation is not measured, release build
//! only, and the numbers are only comparable within one machine.

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ignisium_core::{Document, RopeTextStore, StringTextStore, TextOffset, TextRange};

const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;

/// Sizes used by the groups.
const SIZES: [usize; 3] = [KIB, 64 * KIB, MIB];

fn ascii(size: usize) -> String {
    "a".repeat(size)
}

fn string_document(size: usize) -> Document<StringTextStore> {
    Document::from_text(&ascii(size))
}

fn rope_document(size: usize) -> Document<RopeTextStore> {
    Document::from_store(RopeTextStore::from_text(&ascii(size)))
}

/// Appending at the end of the document.
fn bench_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("backends/append");

    for size in SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("string", size), &size, |b, &size| {
            b.iter_batched(
                || string_document(size),
                |mut document| {
                    document.insert(TextOffset(size), "x").unwrap();

                    // `len` is O(1) for both backends, so the measurement
                    // is the edit and not the read back.
                    black_box(document.len());
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_with_input(BenchmarkId::new("rope", size), &size, |b, &size| {
            b.iter_batched(
                || rope_document(size),
                |mut document| {
                    document.insert(TextOffset(size), "x").unwrap();

                    black_box(document.len());
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}
/// Inserting in the middle: the worst case for a contiguous buffer.
fn bench_middle_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("backends/middle_insert");

    for size in SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("string", size), &size, |b, &size| {
            b.iter_batched(
                || string_document(size),
                |mut document| {
                    document.insert(TextOffset(size / 2), "x").unwrap();

                    black_box(document.len());
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_with_input(BenchmarkId::new("rope", size), &size, |b, &size| {
            b.iter_batched(
                || rope_document(size),
                |mut document| {
                    document.insert(TextOffset(size / 2), "x").unwrap();

                    black_box(document.len());
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

/// Deleting a range in the middle.
fn bench_delete(c: &mut Criterion) {
    let mut group = c.benchmark_group("backends/delete");

    for size in SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("string", size), &size, |b, &size| {
            b.iter_batched(
                || string_document(size),
                |mut document| {
                    document
                        .delete(TextRange::new(
                            TextOffset(size / 3),
                            TextOffset(size / 3 + size / 10),
                        ))
                        .unwrap();

                    black_box(document.len());
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_with_input(BenchmarkId::new("rope", size), &size, |b, &size| {
            b.iter_batched(
                || rope_document(size),
                |mut document| {
                    document
                        .delete(TextRange::new(
                            TextOffset(size / 3),
                            TextOffset(size / 3 + size / 10),
                        ))
                        .unwrap();

                    black_box(document.len());
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

/// Reading a range of the text, the way a viewport or a search window would.
fn bench_read_range(c: &mut Criterion) {
    let mut group = c.benchmark_group("backends/read_range");

    for size in SIZES {
        group.throughput(Throughput::Bytes((size / 10) as u64));

        group.bench_with_input(BenchmarkId::new("string", size), &size, |b, &size| {
            b.iter_batched(
                || string_document(size),
                |document| {
                    let start = TextOffset(size / 2);
                    let end = TextOffset(size / 2 + size / 10);

                    let range = document.text_in(TextRange::new(start, end)).unwrap();

                    black_box(range.len());
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_with_input(BenchmarkId::new("rope", size), &size, |b, &size| {
            b.iter_batched(
                || rope_document(size),
                |document| {
                    let start = TextOffset(size / 2);
                    let end = TextOffset(size / 2 + size / 10);

                    let range = document.text_in(TextRange::new(start, end)).unwrap();

                    black_box(range.len());
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

/// Copying the whole text: the snapshot a background task would take.
///
/// The rope prototype copies the text as well, because it does not share the
/// nodes between snapshots yet (ADR-0009). This benchmark is the one where
/// the prototype still loses.
fn bench_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("backends/snapshot");

    for size in SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("string", size), &size, |b, &size| {
            b.iter_batched(
                || string_document(size),
                |document| {
                    let copy = document.text().into_owned();

                    black_box(copy.len());
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_with_input(BenchmarkId::new("rope", size), &size, |b, &size| {
            b.iter_batched(
                || rope_document(size),
                |document| {
                    let copy = document.text().into_owned();

                    black_box(copy.len());
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_append,
    bench_middle_insert,
    bench_delete,
    bench_read_range,
    bench_snapshot
);
criterion_main!(benches);
