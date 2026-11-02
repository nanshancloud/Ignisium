//! Week 9 and 10: the two storage backends, measured without the setup in the
//! timing.
//!
//! The Criterion benchmarks rebuild the input for every iteration, and that
//! rebuild is part of the measured time: a contiguous backend pays a copy of
//! the whole text, the rope a copy of the whole tree. This probe builds the
//! document **before** the timer starts, so only the operation is measured.
//!
//! The allocation counter below is a global allocator, so the tests of this
//! file must not run at the same time. Run one of them at a time:
//!
//! ```text
//! cargo test --release --test backend_probe backend_comparison -- --ignored --nocapture
//! cargo test --release --test backend_probe memory_comparison -- --ignored --nocapture
//! cargo test --release --test backend_probe read_strategy_comparison -- --ignored --nocapture
//! ```

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ignisium_core::{Document, RopeTextStore, TextOffset, TextRange};

const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;

/// How often an operation is repeated to get a median.
const REPETITIONS: usize = 21;

/// Bytes the probe currently holds, across all allocations.
static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// The highest value `CURRENT` reached since it was reset.
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Counts the bytes the process holds, so that memory can be measured instead
/// of estimated.
struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };

        if !pointer.is_null() {
            let current = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(current, Ordering::Relaxed);
        }

        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);

        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };

        if !new_pointer.is_null() {
            if new_size >= layout.size() {
                let grown = new_size - layout.size();
                let current = CURRENT.fetch_add(grown, Ordering::Relaxed) + grown;
                PEAK.fetch_max(current, Ordering::Relaxed);
            } else {
                CURRENT.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }

        new_pointer
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn current_bytes() -> usize {
    CURRENT.load(Ordering::Relaxed)
}

/// The bytes a document keeps alive, measured after building it.
fn retained<D>(build: impl Fn() -> D) -> usize {
    let before = current_bytes();
    let document = build();
    let after = current_bytes();

    drop(document);

    after.saturating_sub(before)
}

/// The highest number of bytes held while building a document and editing it.
fn peak_of<D>(build: impl Fn() -> D, operation: impl Fn(&mut D)) -> usize {
    let before = current_bytes();

    PEAK.store(before, Ordering::Relaxed);

    let mut document = build();
    operation(&mut document);

    let peak = PEAK.load(Ordering::Relaxed);

    drop(document);

    peak.saturating_sub(before)
}

fn format_bytes(bytes: usize) -> String {
    if bytes < KIB {
        format!("{bytes} B")
    } else if bytes < MIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    }
}

/// Measures one operation on a freshly built document.
///
/// The document is built outside the timer: only the operation is measured.
fn measure<D, B, O>(build: B, operation: O) -> Duration
where
    B: Fn() -> D,
    O: Fn(&mut D),
{
    let mut samples: Vec<Duration> = Vec::with_capacity(REPETITIONS);

    for _ in 0..REPETITIONS {
        let mut document = build();

        let start = Instant::now();
        operation(&mut document);
        samples.push(start.elapsed());
    }

    samples.sort_unstable();

    samples[samples.len() / 2]
}

fn ratio(left: Duration, right: Duration) -> f64 {
    left.as_secs_f64() / right.as_secs_f64()
}

fn format_time(duration: Duration) -> String {
    let nanos = duration.as_nanos();

    if nanos < 10_000 {
        format!("{:.0} ns", nanos as f64)
    } else if nanos < 1_000_000 {
        format!("{:.2} us", nanos as f64 / 1_000.0)
    } else {
        format!("{:.1} us", nanos as f64 / 1_000.0)
    }
}
#[test]
#[ignore = "allocates up to 10 MiB, run it explicitly"]
fn backend_comparison() {
    println!();
    println!(
        "backend comparison (release build, median of {} runs)",
        REPETITIONS
    );
    println!();
    println!(
        "{:>8}  {:<14}  {:>12}  {:>12}  {:>8}",
        "size", "operation", "string", "rope", "ratio"
    );

    for size in [KIB, 64 * KIB, MIB, 10 * MIB, 100 * MIB] {
        let text = "a".repeat(size);
        let middle = size / 2;
        let third = size / 3;

        // append one byte at the end
        let string = measure(
            || Document::from_text(&text),
            |document| {
                document.insert(TextOffset(size), "x").unwrap();
            },
        );
        let rope = measure(
            || Document::from_store(RopeTextStore::from_text(&text)),
            |document| {
                document.insert(TextOffset(size), "x").unwrap();
            },
        );
        println!(
            "{:>7} KiB  {:<14}  {:>12}  {:>12}  {:>7.2}x",
            size / KIB,
            "append",
            format_time(string),
            format_time(rope),
            ratio(string, rope)
        );

        // insert one byte in the middle
        let string = measure(
            || Document::from_text(&text),
            |document| {
                document.insert(TextOffset(middle), "x").unwrap();
            },
        );
        let rope = measure(
            || Document::from_store(RopeTextStore::from_text(&text)),
            |document| {
                document.insert(TextOffset(middle), "x").unwrap();
            },
        );
        println!(
            "{:>7} KiB  {:<14}  {:>12}  {:>12}  {:>7.2}x",
            size / KIB,
            "middle insert",
            format_time(string),
            format_time(rope),
            ratio(string, rope)
        );

        // delete a tenth of the document
        let string = measure(
            || Document::from_text(&text),
            |document| {
                document
                    .delete(TextRange::new(
                        TextOffset(third),
                        TextOffset(third + size / 10),
                    ))
                    .unwrap();
            },
        );
        let rope = measure(
            || Document::from_store(RopeTextStore::from_text(&text)),
            |document| {
                document
                    .delete(TextRange::new(
                        TextOffset(third),
                        TextOffset(third + size / 10),
                    ))
                    .unwrap();
            },
        );
        println!(
            "{:>7} KiB  {:<14}  {:>12}  {:>12}  {:>7.2}x",
            size / KIB,
            "delete 10%",
            format_time(string),
            format_time(rope),
            ratio(string, rope)
        );

        // read a tenth of the document
        let string = measure(
            || Document::from_text(&text),
            |document| {
                let range = document
                    .text_in(TextRange::new(
                        TextOffset(middle),
                        TextOffset(middle + size / 10),
                    ))
                    .unwrap();

                assert_eq!(range.len(), size / 10);
            },
        );
        let rope = measure(
            || Document::from_store(RopeTextStore::from_text(&text)),
            |document| {
                let range = document
                    .text_in(TextRange::new(
                        TextOffset(middle),
                        TextOffset(middle + size / 10),
                    ))
                    .unwrap();

                assert_eq!(range.len(), size / 10);
            },
        );
        println!(
            "{:>7} KiB  {:<14}  {:>12}  {:>12}  {:>7.2}x",
            size / KIB,
            "read 10%",
            format_time(string),
            format_time(rope),
            ratio(string, rope)
        );

        // copy the whole text: the snapshot a background task would take
        let string = measure(
            || Document::from_text(&text),
            |document| {
                let copy = document.text().into_owned();

                assert_eq!(copy.len(), size);
            },
        );
        let rope = measure(
            || Document::from_store(RopeTextStore::from_text(&text)),
            |document| {
                let copy = document.text().into_owned();

                assert_eq!(copy.len(), size);
            },
        );
        println!(
            "{:>7} KiB  {:<14}  {:>12}  {:>12}  {:>7.2}x",
            size / KIB,
            "snapshot",
            format_time(string),
            format_time(rope),
            ratio(string, rope)
        );

        println!();
    }
}

/// Retained and peak memory of the two backends (gap G9 of the week 9 audit).
#[test]
#[ignore = "allocates up to 100 MiB, run it alone"]
fn memory_comparison() {
    println!();
    println!("memory (release build, retained after construction / peak of one append)");
    println!();
    println!(
        "{:>9}  {:>12}  {:>12}  {:>9}  {:>12}  {:>12}  {:>9}",
        "size", "string", "rope", "rope/str", "string peak", "rope peak", "rope/str"
    );

    for size in [KIB, 64 * KIB, MIB, 10 * MIB, 100 * MIB] {
        let text = "a".repeat(size);

        let string = retained(|| Document::from_text(&text));
        let rope = retained(|| Document::from_store(RopeTextStore::from_text(&text)));

        let string_peak = peak_of(
            || Document::from_text(&text),
            |document| {
                document.insert(TextOffset(size / 2), "x").unwrap();
            },
        );
        let rope_peak = peak_of(
            || Document::from_store(RopeTextStore::from_text(&text)),
            |document| {
                document.insert(TextOffset(size / 2), "x").unwrap();
            },
        );

        println!(
            "{:>8} K  {:>12}  {:>12}  {:>8.2}x  {:>12}  {:>12}  {:>8.2}x",
            size / KIB,
            format_bytes(string),
            format_bytes(rope),
            rope as f64 / string as f64,
            format_bytes(string_peak),
            format_bytes(rope_peak),
            rope_peak as f64 / string_peak as f64
        );
    }

    println!();
}

/// `text_in` copies the range on a chunked backend, `for_each_in` does not.
/// This is the measurement that decides the read strategy (G1).
#[test]
#[ignore = "allocates up to 100 MiB, run it alone"]
fn read_strategy_comparison() {
    println!();
    println!(
        "reading a tenth of the document (release build, median of {} runs)",
        REPETITIONS
    );
    println!();
    println!(
        "{:>9}  {:>14}  {:>12}  {:>12}  {:>10}",
        "size", "backend/read", "time", "copied", "visits"
    );

    for size in [MIB, 10 * MIB, 100 * MIB] {
        let text = "a".repeat(size);
        let middle = size / 2;
        let range = TextRange::new(TextOffset(middle), TextOffset(middle + size / 10));

        // Borrowing a slice: the string backend copies nothing.
        let string = measure(
            || Document::from_text(&text),
            |document| {
                let piece = document.text_in(range).unwrap();

                assert_eq!(piece.len(), size / 10);
            },
        );
        println!(
            "{:>8} K  {:<14}  {:>12}  {:>12}  {:>10}",
            size / KIB,
            "string text_in",
            format_time(string),
            format_bytes(0),
            "-"
        );

        // Copying the range out of the chunks.
        let rope = measure(
            || Document::from_store(RopeTextStore::from_text(&text)),
            |document| {
                let piece = document.text_in(range).unwrap();

                assert_eq!(piece.len(), size / 10);
            },
        );
        println!(
            "{:>8} K  {:<14}  {:>12}  {:>12}  {:>10}",
            size / KIB,
            "rope text_in",
            format_time(rope),
            format_bytes(size / 10),
            "-"
        );

        // Visiting the chunks: no allocation at all.
        let visited = measure(
            || Document::from_store(RopeTextStore::from_text(&text)),
            |document| {
                let mut total = 0;

                document
                    .for_each_in(range, &mut |piece| total += piece.len())
                    .unwrap();

                assert_eq!(total, size / 10);
            },
        );
        let chunk_count = (size / 10).div_ceil(KIB);

        println!(
            "{:>8} K  {:<14}  {:>12}  {:>12}  {:>10}",
            size / KIB,
            "rope for_each_in",
            format_time(visited),
            format_bytes(0),
            chunk_count
        );

        println!();
    }
}
