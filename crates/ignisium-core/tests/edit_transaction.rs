//! Week 6: edit transactions, seen from the outside.
//!
//! The unit tests in `src/document.rs` cover the rules one by one. This file
//! walks through the situations a host actually runs into.

use ignisium_core::{
    Document, EditProblem, RevisionId, TextEdit, TextError, TextOffset, TextRange,
};

fn document_with(text: &str) -> Document {
    Document::from_text(text)
}

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(TextOffset(start), TextOffset(end))
}

/// Pressing enter on an indented line: one user action, one transaction,
/// one revision. The batch uses a single `replace`, because an insertion at
/// the same offset as another edit's range would be rejected as
/// overlapping.
#[test]
fn auto_indent_is_one_transaction() {
    let mut document = document_with("    return 1;");

    let revision = document
        .apply_edits(&[TextEdit::replace(range(0, 4), "\n    ")])
        .expect("valid batch");

    assert_eq!(document.text(), "\n    return 1;");
    assert_eq!(revision, RevisionId::INITIAL.next());
}

/// Several cursors: every edit is written against the text as it was before
/// the transaction, so the host does not have to shift offsets.
#[test]
fn several_cursors_edit_in_one_transaction() {
    let mut document = document_with("one two three");

    document
        .apply_edits(&[
            TextEdit::replace(range(0, 3), "1"),
            TextEdit::replace(range(4, 7), "2"),
            TextEdit::replace(range(8, 13), "3"),
        ])
        .unwrap();

    assert_eq!(document.text(), "1 2 3");
}

/// Multi-cursor delete: every selected range disappears in one transaction.
#[test]
fn deletions_across_the_document() {
    let mut document = document_with("one two three");

    document
        .apply_edits(&[
            TextEdit::delete(range(0, 4)),
            TextEdit::delete(range(7, 13)),
        ])
        .unwrap();

    assert_eq!(document.text(), "two");
}

/// An insertion right behind a removed range keeps its meaning: the offset
/// still refers to the text as it was before the transaction.
#[test]
fn an_insertion_after_a_deleted_range() {
    let mut document = document_with("a1b2c");

    document
        .apply_edits(&[
            TextEdit::delete(range(1, 2)),
            TextEdit::insert(TextOffset(2), "X"),
        ])
        .unwrap();

    assert_eq!(document.text(), "aXb2c");
}

/// A batch built against an older revision is not applied half-way: the
/// document keeps the old text and the old revision.
#[test]
fn a_stale_batch_is_rejected_as_a_whole() {
    let mut document = document_with("abc");

    document
        .apply_edits(&[TextEdit::insert(TextOffset(3), "!")])
        .unwrap();
    let revision = document.revision();

    let result = document.apply_edits(&[
        TextEdit::replace(range(0, 3), "xyz"),
        TextEdit::delete(range(3, 99)),
    ]);

    assert_eq!(
        result,
        Err(TextError::OffsetOutOfRange { offset: 99, len: 4 })
    );
    assert_eq!(document.text(), "abc!");
    assert_eq!(document.revision(), revision);
}

/// Chinese text: a transaction may only cut on character boundaries.
#[test]
fn transactions_on_chinese_text() {
    let mut document = document_with("你好，世界");

    document
        .apply_edits(&[
            TextEdit::delete(range(0, 3)),
            TextEdit::insert(TextOffset(15), "！"),
        ])
        .unwrap();

    assert_eq!(document.text(), "好，世界！");
}

#[test]
fn a_transaction_never_splits_a_character() {
    let mut document = document_with("你好");

    let result = document.apply_edits(&[
        TextEdit::delete(range(3, 6)),
        TextEdit::insert(TextOffset(1), "x"),
    ]);

    assert_eq!(result, Err(TextError::NotCharBoundary { offset: 1 }));
    assert_eq!(document.text(), "你好");
}

/// The host asks what is wrong before it applies anything, so it can report
/// every bad edit.
#[test]
fn a_host_can_inspect_a_batch_before_applying_it() {
    let document = document_with("abc");

    let problems = document.edit_problems(&[
        TextEdit::insert(TextOffset(0), "x"),
        TextEdit::insert(TextOffset(9), "y"),
        TextEdit::delete(range(0, 2)),
    ]);

    assert_eq!(
        problems,
        vec![
            EditProblem {
                index: 1,
                error: TextError::OffsetOutOfRange { offset: 9, len: 3 },
            },
            EditProblem {
                index: 2,
                error: TextError::OverlappingEdits {
                    first: 0,
                    second: 2
                },
            },
        ]
    );
}

/// Editing a read-only document fails as a transaction, not as a partial edit.
#[test]
fn a_read_only_document_refuses_the_whole_batch() {
    let mut document = document_with("abc");
    document.set_read_only(true);

    let result = document.apply_edits(&[TextEdit::insert(TextOffset(3), "!")]);

    assert_eq!(result, Err(TextError::ReadOnly));
    assert_eq!(document.text(), "abc");
    assert_eq!(document.revision(), RevisionId::INITIAL);
}
