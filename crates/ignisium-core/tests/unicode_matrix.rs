//! Week 4: Unicode position test matrix.
//!
//! The matrix records, for each sample, the numbers that matter when talking
//! about "a position in text":
//!
//! - `bytes`: UTF-8 byte length, the unit `TextOffset` counts today
//! - `chars`: Unicode scalar values (Rust `char`)
//! - `graphemes`: user-perceived characters (UAX#29)
//! - `boundaries`: byte offsets that start a scalar value
//!
//! `graphemes` cannot be computed with the standard library today, so the
//! values are recorded by hand. They exist to keep the gap between "what the
//! storage layer counts" and "what a user sees" visible and tested. See
//! `docs/design/text-position.md`.

use ignisium_core::{Document, TextError, TextOffset, TextRange};

struct Sample {
    label: &'static str,
    text: &'static str,
    bytes: usize,
    chars: usize,
    graphemes: usize,
    boundaries: &'static [usize],
}

const SAMPLES: &[Sample] = &[
    Sample {
        label: "ascii",
        text: "Hello",
        bytes: 5,
        chars: 5,
        graphemes: 5,
        boundaries: &[0, 1, 2, 3, 4, 5],
    },
    Sample {
        label: "cjk",
        text: "你好",
        bytes: 6,
        chars: 2,
        graphemes: 2,
        boundaries: &[0, 3, 6],
    },
    Sample {
        label: "mixed_ascii_cjk",
        text: "Hello, 世界",
        bytes: 13,
        chars: 9,
        graphemes: 9,
        boundaries: &[0, 1, 2, 3, 4, 5, 6, 7, 10, 13],
    },
    Sample {
        // Precomposed U+00E9 (NFC).
        label: "cafe_nfc",
        text: "caf\u{00e9}",
        bytes: 5,
        chars: 4,
        graphemes: 4,
        boundaries: &[0, 1, 2, 3, 5],
    },
    Sample {
        // "e" + combining acute U+0301 (NFD): same visible text, longer.
        label: "cafe_nfd",
        text: "cafe\u{0301}",
        bytes: 6,
        chars: 5,
        graphemes: 4,
        boundaries: &[0, 1, 2, 3, 4, 6],
    },
    Sample {
        label: "combining_mark",
        text: "a\u{0301}",
        bytes: 3,
        chars: 2,
        graphemes: 1,
        boundaries: &[0, 1, 3],
    },
    Sample {
        label: "emoji",
        text: "\u{1f44d}",
        bytes: 4,
        chars: 1,
        graphemes: 1,
        boundaries: &[0, 4],
    },
    Sample {
        // Emoji + skin tone modifier: two scalars, one grapheme.
        label: "emoji_skin_tone",
        text: "\u{1f44d}\u{1f3fd}",
        bytes: 8,
        chars: 2,
        graphemes: 1,
        boundaries: &[0, 4, 8],
    },
    Sample {
        // Family: man + ZWJ + woman + ZWJ + girl.
        label: "emoji_zwj_family",
        text: "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}",
        bytes: 18,
        chars: 5,
        graphemes: 1,
        boundaries: &[0, 4, 7, 11, 14, 18],
    },
    Sample {
        // Regional indicator pair (flag).
        label: "flag",
        text: "\u{1f1fa}\u{1f1f8}",
        bytes: 8,
        chars: 2,
        graphemes: 1,
        boundaries: &[0, 4, 8],
    },
    Sample {
        // CRLF is one grapheme cluster (UAX#29 rule GB3) but two bytes.
        label: "crlf",
        text: "line1\r\nline2",
        bytes: 12,
        chars: 12,
        graphemes: 11,
        boundaries: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
    },
    Sample {
        label: "tab",
        text: "a\tb",
        bytes: 3,
        chars: 3,
        graphemes: 3,
        boundaries: &[0, 1, 2, 3],
    },
];

fn document_with(text: &str) -> Document {
    let mut document = Document::new();
    document.insert(TextOffset(0), text).unwrap();
    document
}

#[test]
fn recorded_measurements_match_the_document() {
    for sample in SAMPLES {
        let document = document_with(sample.text);

        assert_eq!(document.text(), sample.text, "{}: round trip", sample.label);
        assert_eq!(
            document.len(),
            sample.bytes,
            "{}: byte length",
            sample.label
        );
        assert_eq!(
            document.text().chars().count(),
            sample.chars,
            "{}: scalar count",
            sample.label
        );
    }
}

#[test]
fn recorded_boundaries_match_is_char_boundary() {
    for sample in SAMPLES {
        let document = document_with(sample.text);

        for offset in 0..=sample.bytes {
            let expected = sample.boundaries.contains(&offset);

            assert_eq!(
                document.is_char_boundary(TextOffset(offset)),
                expected,
                "{}: offset {}",
                sample.label,
                offset
            );
        }
    }
}

#[test]
fn insert_at_every_boundary_matches_slicing() {
    for sample in SAMPLES {
        for &offset in sample.boundaries {
            let mut document = document_with(sample.text);

            document.insert(TextOffset(offset), "X").unwrap();

            let expected = format!("{}X{}", &sample.text[..offset], &sample.text[offset..]);
            assert_eq!(
                document.text(),
                expected,
                "{}: insert at {}",
                sample.label,
                offset
            );
        }
    }
}

#[test]
fn insert_inside_a_scalar_returns_not_char_boundary() {
    for sample in SAMPLES {
        for offset in 0..sample.bytes {
            if sample.boundaries.contains(&offset) {
                continue;
            }

            let mut document = document_with(sample.text);

            assert_eq!(
                document.insert(TextOffset(offset), "X"),
                Err(TextError::NotCharBoundary { offset }),
                "{}: offset {}",
                sample.label,
                offset
            );
            assert_eq!(document.text(), sample.text, "{}: unchanged", sample.label);
        }
    }
}

#[test]
fn insert_past_the_end_returns_offset_out_of_range() {
    for sample in SAMPLES {
        let mut document = document_with(sample.text);
        let offset = sample.bytes + 1;

        assert_eq!(
            document.insert(TextOffset(offset), "X"),
            Err(TextError::OffsetOutOfRange {
                offset,
                len: sample.bytes
            }),
            "{}",
            sample.label
        );
        assert_eq!(document.text(), sample.text);
    }
}

#[test]
fn delete_the_whole_sample_empties_the_document() {
    for sample in SAMPLES {
        let mut document = document_with(sample.text);

        document
            .delete(TextRange::new(TextOffset(0), TextOffset(sample.bytes)))
            .unwrap();

        assert_eq!(document.text(), "", "{}", sample.label);
        assert_eq!(document.len(), 0, "{}", sample.label);
        assert!(document.is_empty(), "{}", sample.label);
    }
}

// The gap this matrix exists to document: scalars are not what a user sees.
// For these samples the grapheme count differs from the scalar count, and the
// standard library has no way to compute it (UAX#29 data is required).
#[test]
fn scalars_are_not_graphemes() {
    let divergent: Vec<&Sample> = SAMPLES
        .iter()
        .filter(|sample| sample.chars != sample.graphemes)
        .collect();

    assert!(!divergent.is_empty());

    for sample in divergent {
        assert_eq!(
            document_with(sample.text).text().chars().count(),
            sample.chars,
            "{}",
            sample.label
        );
        assert_ne!(
            sample.chars, sample.graphemes,
            "{}: expected the counts to diverge",
            sample.label
        );
    }
}

// A boundary inside a grapheme is still a legal byte offset. The emoji with a
// skin tone modifier can be split at offset 4 even though the user sees one
// character. This is the current, documented behavior.
#[test]
fn byte_offsets_may_split_a_grapheme() {
    let sample = Sample {
        label: "emoji_skin_tone",
        text: "\u{1f44d}\u{1f3fd}",
        bytes: 8,
        chars: 2,
        graphemes: 1,
        boundaries: &[0, 4, 8],
    };

    let mut document = document_with(sample.text);
    document
        .delete(TextRange::new(TextOffset(4), TextOffset(8)))
        .unwrap();

    assert_eq!(document.text(), "\u{1f44d}");
    assert_eq!(document.len(), 4);
}
