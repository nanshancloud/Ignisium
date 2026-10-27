//! Week 9: the two storage backends, measured without the setup in the timing.
//!
//! The Criterion benchmarks rebuild the input for every iteration, and that
//! rebuild is part of the measured time: a contiguous backend pays a copy of
//! the whole text, the rope a copy of the whole tree. This probe builds the
//! document **before** the timer starts, so only the operation is measured.
//!
//! The test is ignored, because it allocates up to 10 MiB and repeats every
//! measurement. Run it explicitly:
//!
//! ```text
//! cargo test --release --test backend_probe -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use ignisium_core::{Document, RopeTextStore, TextOffset, TextRange};

const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;

/// How often an operation is repeated to get a median.
const REPETITIONS: usize = 21;

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

    for size in [KIB, 64 * KIB, MIB, 10 * MIB] {
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
