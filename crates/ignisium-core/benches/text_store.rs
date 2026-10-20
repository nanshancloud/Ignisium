//! Week 7: benchmarks for the current text storage.
//!
//! These benchmarks measure the `StringTextStore` backend, the only one that
//! exists today. Their purpose is not to prove that it is fast, but to give
//! week 9 and week 10 a baseline: a second backend (Rope) is only worth its
//! complexity if it beats the numbers measured here.
//!
//! Run them with:
//!
//! ```text
//! cargo bench --bench text_store
//! ```
//!
//! Every benchmark measures **one edit on a prepared document**. Building the
//! document is setup work and is not part of the measurement, otherwise the
//! numbers would describe the allocator rather than the storage layer.

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ignisium_core::{Document, TextOffset, TextRange};

const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;

/// Sizes used by the small and medium groups.
const SIZES: [usize; 3] = [KIB, 64 * KIB, MIB];

fn ascii(size: usize) -> String {
    "a".repeat(size)
}

fn unicode(size: usize) -> String {
    "中".repeat(size)
}

/// Appending at the end of the document.
fn bench_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("append");

    for size in SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || Document::from_text(&ascii(size)),
                |mut document| {
                    document.insert(TextOffset(size), "x").unwrap();

                    black_box(document.text().len());
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

/// Inserting in the middle: the worst case for a contiguous buffer, because the
/// whole tail has to be moved.
fn bench_middle_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("middle_insert");

    for size in SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || Document::from_text(&ascii(size)),
                |mut document| {
                    document.insert(TextOffset(size / 2), "x").unwrap();

                    black_box(document.text().len());
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

/// Deleting a range in the middle.
fn bench_delete(c: &mut Criterion) {
    let mut group = c.benchmark_group("delete");

    for size in SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || Document::from_text(&ascii(size)),
                |mut document| {
                    let start = TextOffset(size / 3);
                    let end = TextOffset(size / 3 + size / 10);

                    document.delete(TextRange::new(start, end)).unwrap();

                    black_box(document.text().len());
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

/// Editing text that is not ASCII: the same character count, three times the
/// bytes, and a different memory layout.
fn bench_unicode(c: &mut Criterion) {
    let mut group = c.benchmark_group("unicode");

    for size in [KIB, 64 * KIB] {
        group.throughput(Throughput::Bytes((size * 3) as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || Document::from_text(&unicode(size)),
                |mut document| {
                    document.insert(TextOffset(size * 3), "中").unwrap();

                    black_box(document.text().len());
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

/// A ten megabyte document: the size where a contiguous buffer starts to hurt,
/// and the size the long line benchmark (week 12) will use as a reference.
fn bench_large_document(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_document");
    let size = 10 * MIB;

    group.sample_size(10);
    group.throughput(Throughput::Bytes(size as u64));

    group.bench_function("append", |b| {
        b.iter_batched(
            || Document::from_text(&ascii(size)),
            |mut document| {
                document.insert(TextOffset(size), "x").unwrap();

                black_box(document.text().len());
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("middle_insert", |b| {
        b.iter_batched(
            || Document::from_text(&ascii(size)),
            |mut document| {
                document.insert(TextOffset(size / 2), "x").unwrap();

                black_box(document.text().len());
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_append,
    bench_middle_insert,
    bench_delete,
    bench_unicode,
    bench_large_document
);
criterion_main!(benches);
