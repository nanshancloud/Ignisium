//! Week 11: one suite, every backend (G8).
//!
//! "The two backends behave the same" is a test, not a statement. Everything
//! in `suite` runs once per backend, so a third backend is added by naming its
//! constructor and nothing else - and a difference between two backends shows
//! up as a failure of the suite, not as a bug a host finds later.
//!
//! The second half is the contract the edit pipeline leans on (G12): once
//! `validate_offset` / `validate_range` has accepted a position, the call
//! succeeds. A backend that breaks it would make a batch fail after some of
//! its edits were already applied, which contradicts "all or nothing".

use std::ops::ControlFlow;

use ignisium_core::{
    Document, EditBatch, EditProblem, RevisionId, RopeTextStore, StringTextStore, TextEdit,
    TextError, TextOffset, TextRange, TextStore,
};

const TEXT: &str = "Hello 世界, one two three";

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(TextOffset(start), TextOffset(end))
}

/// Everything the two backends have to agree on.
fn suite<S: TextStore>(load: fn(&str) -> S) {
    // -- reading ---------------------------------------------------------

    let document = Document::from_store(load(TEXT));

    assert_eq!(document.len(), TEXT.len());
    assert!(!document.is_empty());
    assert_eq!(document.text(), TEXT);
    assert_eq!(document.text_in(range(0, TEXT.len())).unwrap(), TEXT);
    assert_eq!(document.text_in(range(6, 12)).unwrap(), "世界");
    assert_eq!(document.text_in(range(3, 3)).unwrap(), "");

    assert_eq!(
        document.text_in(range(0, TEXT.len() + 1)),
        Err(TextError::OffsetOutOfRange {
            offset: TEXT.len() + 1,
            len: TEXT.len(),
        })
    );
    assert_eq!(
        document.text_in(range(1, 0)),
        Err(TextError::InvalidRange { start: 1, end: 0 })
    );

    // -- the non-copying read agrees with the copying one ----------------

    for current in [range(0, TEXT.len()), range(6, 12), range(3, 3), range(0, 1)] {
        let expected = document.text_in(current).unwrap();
        let mut pieces = 0;
        let mut collected = String::new();

        document
            .for_each_in(current, &mut |piece| {
                pieces += 1;
                collected.push_str(piece);

                ControlFlow::Continue(())
            })
            .unwrap();

        assert_eq!(
            collected, expected,
            "{current:?} read differently piece by piece"
        );
        // A non-empty range is visited at least once; an empty one may be
        // visited zero or one time, with nothing.
        assert!(pieces >= 1 || current.is_empty());

        // A stop is not an error, and it stops at the first piece.
        let mut visited = 0;
        let outcome = document.for_each_in(current, &mut |_| {
            visited += 1;

            ControlFlow::Break(())
        });

        assert_eq!(outcome, Ok(()));
        assert!(visited <= 1);

        if !current.is_empty() {
            assert_eq!(visited, 1);
        }
    }

    // Offset 7 sits inside the three bytes of "世".
    assert_eq!(
        document.for_each_in(range(0, 7), &mut |_| ControlFlow::Continue(())),
        Err(TextError::NotCharBoundary { offset: 7 })
    );

    // -- one edit is one transaction -------------------------------------

    let mut document = Document::from_store(load(""));
    let first = RevisionId::INITIAL.next();

    assert_eq!(document.insert(TextOffset(0), "Hello"), Ok(first));
    assert_eq!(document.text(), "Hello");
    assert_eq!(document.revision().as_u64(), 1);

    document.insert(TextOffset(5), " 世界").unwrap();
    assert_eq!(document.text(), "Hello 世界");

    document.delete(range(0, 2)).unwrap();
    assert_eq!(document.text(), "llo 世界");

    // "llo 世界": offset 5 is inside "世".
    assert_eq!(document.text(), "llo 世界");
    assert_eq!(
        document.insert(TextOffset(5), "x"),
        Err(TextError::NotCharBoundary { offset: 5 })
    );
    assert_eq!(
        document.insert(TextOffset(99), "x"),
        Err(TextError::OffsetOutOfRange {
            offset: 99,
            len: 10
        })
    );
    assert_eq!(
        document.delete(range(3, 1)),
        Err(TextError::InvalidRange { start: 3, end: 1 })
    );

    // -- transactions ----------------------------------------------------

    let mut document = Document::from_store(load("one two three"));

    document
        .apply_edits(&[
            TextEdit::replace(range(0, 3), "1"),
            TextEdit::replace(range(4, 7), "2"),
            TextEdit::replace(range(8, 13), "3"),
        ])
        .unwrap();

    assert_eq!(document.text(), "1 2 3");
    assert_eq!(document.revision().as_u64(), 1);

    let mut document = Document::from_store(load("abcdef"));

    assert_eq!(
        document.apply_edits(&[TextEdit::delete(range(0, 4)), TextEdit::delete(range(2, 6)),]),
        Err(TextError::OverlappingEdits {
            first: 0,
            second: 1
        })
    );
    assert_eq!(document.text(), "abcdef");
    assert_eq!(document.revision().as_u64(), 0);

    // Two insertions at one offset are ordered, not rejected, and both
    // spellings of an insertion are the same edit (ADR-0013 decision 2).
    let mut document = Document::from_store(load("abc"));

    document
        .apply_edits(&[
            TextEdit::insert(TextOffset(1), "X"),
            TextEdit::replace(range(1, 1), "Y"),
        ])
        .unwrap();

    assert_eq!(document.text(), "aXYbc");
    assert_eq!(document.revision().as_u64(), 1);

    // -- the revision guard and the map ----------------------------------

    let mut document = Document::from_store(load("one two three"));

    let applied = document
        .apply_batch(&EditBatch::new(
            document.revision(),
            vec![
                TextEdit::replace(range(0, 3), "1"),
                TextEdit::replace(range(8, 13), "3"),
            ],
        ))
        .unwrap();

    assert_eq!(document.text(), "1 two 3");
    assert_eq!(applied.revision, document.revision());
    assert_eq!(applied.map.map(TextOffset(5)), TextOffset(3));

    assert_eq!(
        document.apply_batch(&EditBatch::new(
            document.revision().next(),
            vec![TextEdit::insert(TextOffset(0), "!")],
        )),
        Err(TextError::StaleRevision {
            expected: document.revision().next(),
            actual: document.revision(),
        })
    );
    assert_eq!(document.text(), "1 two 3");

    // -- metadata --------------------------------------------------------

    let mut document = Document::from_store(load("abc"));
    document.set_read_only(true);

    assert_eq!(
        document.apply_edits(&[TextEdit::insert(TextOffset(3), "!")]),
        Err(TextError::ReadOnly)
    );
    assert_eq!(document.text(), "abc");

    // -- validated means infallible (G12) --------------------------------

    validated_edits_never_fail(load);
    a_checked_batch_applies(load);
}

/// Once the position model has accepted a range, the backend does not refuse
/// it. Every valid position of a small text is tried, on a fresh store each
/// time, so a failure names the exact position that broke the contract.
fn validated_edits_never_fail<S: TextStore>(load: fn(&str) -> S) {
    let text = "a中b文c";

    for start in 0..=text.len() {
        if !text.is_char_boundary(start) {
            continue;
        }

        let mut store = load(text);
        let offset = TextOffset(start);

        assert_eq!(store.validate_offset(offset), Ok(()));
        assert_eq!(
            store.insert(offset, "x"),
            Ok(()),
            "insert at {start} was validated and then refused"
        );

        for end in start..=text.len() {
            if !text.is_char_boundary(end) {
                continue;
            }

            let mut store = load(text);
            let current = range(start, end);

            assert_eq!(store.validate_range(current), Ok(()));
            assert_eq!(
                store.delete(current),
                Ok(()),
                "delete of {current:?} was validated and then refused"
            );
        }
    }
}

/// The same contract, seen from the pipeline: a batch that `edit_problems`
/// accepts is a batch `apply_edits` applies, whole.
fn a_checked_batch_applies<S: TextStore>(load: fn(&str) -> S) {
    let mut document = Document::from_store(load("one two three"));

    let edits = vec![
        TextEdit::replace(range(0, 3), "1"),
        TextEdit::insert(TextOffset(3), ","),
        TextEdit::replace(range(8, 13), "3"),
    ];

    assert_eq!(document.edit_problems(&edits), Vec::<EditProblem>::new());
    document.apply_edits(&edits).unwrap();

    assert_eq!(document.text(), "1, two 3");
    assert_eq!(document.revision().as_u64(), 1);
}

#[test]
fn the_string_backend_conforms() {
    suite(StringTextStore::from_text);
}

#[test]
fn the_rope_backend_conforms() {
    suite(RopeTextStore::from_text);
}

/// Equality of a store, and of a document, is about the text, not about how
/// it is stored (ADR-0012 decision 7).
#[test]
fn two_rope_stores_with_the_same_text_are_equal() {
    let mut spread = RopeTextStore::new();

    for _ in 0..1100 {
        spread.insert(TextOffset(0), "a").unwrap();
    }

    let packed = RopeTextStore::from_text(&"a".repeat(1100));

    assert_ne!(spread.rope().leaf_count(), packed.rope().leaf_count());
    assert_eq!(spread, packed);
}

#[test]
fn a_document_reports_whether_the_text_changed() {
    let mut edited = Document::from_text("abc");
    let untouched = Document::from_text("abc");

    edited.insert(TextOffset(0), "x").unwrap();
    edited.delete(range(0, 1)).unwrap();

    // Same characters, different history: `text_eq` says "no text change",
    // `==` says "not the same state".
    assert_eq!(edited.text(), untouched.text());
    assert!(edited.text_eq(&untouched));
    assert_ne!(edited, untouched);
}
