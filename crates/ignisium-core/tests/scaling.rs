//! Week 7: large document scaling probe.
//!
//! Answers a different question than the Criterion benchmarks: how does the
//! backend behave as the document grows?
//!
//! The test is ignored on purpose: allocating 100 MiB is far too slow for an
//! ordinary test run. Run it explicitly:
//!
//! ```text
//! cargo test --release --test scaling -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use ignisium_core::{Document, TextOffset, TextRange};

const MIB: usize = 1024 * 1024;

struct Measurement {
    label: &'static str,
    elapsed: Duration,
    bytes: usize,
}

impl Measurement {
    fn new(label: &'static str, elapsed: Duration, bytes: usize) -> Self {
        Self {
            label,
            elapsed,
            bytes,
        }
    }

    fn mib_per_second(&self) -> f64 {
        let seconds = self.elapsed.as_secs_f64();

        if seconds == 0.0 {
            return f64::INFINITY;
        }

        (self.bytes as f64 / MIB as f64) / seconds
    }
}

fn time<F: FnOnce()>(f: F) -> Duration {
    let start = Instant::now();

    f();

    start.elapsed()
}

fn measure(size: usize) -> Vec<Measurement> {
    let text = "a".repeat(size);
    let mut measurements = Vec::new();

    let load = time(|| {
        let document = Document::from_text(&text);

        assert_eq!(document.len(), size);
    });
    measurements.push(Measurement::new("load", load, size));

    let mut document = Document::from_text(&text);

    let append = time(|| {
        document.insert(TextOffset(size), "x").unwrap();
    });
    measurements.push(Measurement::new("append 1 byte", append, size));

    let mut document = Document::from_text(&text);

    let middle = time(|| {
        document.insert(TextOffset(size / 2), "x").unwrap();
    });
    measurements.push(Measurement::new("middle insert 1 byte", middle, size));

    let mut document = Document::from_text(&text);
    let delete_end = size / 3 + size / 10;

    let delete = time(|| {
        document
            .delete(TextRange::new(TextOffset(size / 3), TextOffset(delete_end)))
            .unwrap();
    });
    measurements.push(Measurement::new("delete 10%", delete, size / 10));

    measurements
}

#[test]
#[ignore = "allocates up to 100 MiB, run it explicitly"]
fn scaling_probe() {
    println!();
    println!("document scaling probe (release build)");
    println!(
        "{:>8}  {:<22} {:>12}  {:>14}",
        "size", "operation", "time", "throughput"
    );

    for size in [MIB, 10 * MIB, 100 * MIB] {
        for measurement in measure(size) {
            println!(
                "{:>7} MiB  {:<22} {:>12}  {:>11.1} MiB/s",
                size / MIB,
                measurement.label,
                format!("{:?}", measurement.elapsed),
                measurement.mib_per_second()
            );
        }
    }

    println!();
}
