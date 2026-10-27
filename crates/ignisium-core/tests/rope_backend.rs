//! Week 9: the rope backend, seen from the outside.
//!
//! The rope is the second storage backend. The point of these tests is that
//! the document layer does not know which backend it is talking to: the same
//! edits, the same transactions and the same errors have to work on both.

use ignisium_core::{
    Document, RopeTextStore, StringTextStore, TextEdit, TextError, TextOffset, TextRange,
};

fn rope_document(text: &str) -> Document<RopeTextStore> {
    Document::from_store(RopeTextStore::from_text(text))
}

fn string_document(text: &str) -> Document<StringTextStore> {
    Document::from_text(text)
}

#[test]
fn a_document_can_run_on_a_rope() {
    let mut document = rope_document("Hello");

    document.insert(TextOffset(5), " world").unwrap();

    assert_eq!(document.text(), "Hello world");
    assert_eq!(document.len(), 11);
    assert_eq!(document.revision().as_u64(), 1);
}

#[test]
fn a_range_of_a_rope_document_is_read_without_the_rest() {
    let document = rope_document("Hello world");

    assert_eq!(
        document
            .text_in(TextRange::new(TextOffset(0), TextOffset(5)))
            .unwrap(),
        "Hello"
    );
    assert_eq!(
        document
            .text_in(TextRange::new(TextOffset(6), TextOffset(11)))
            .unwrap(),
        "world"
    );
}

#[test]
fn transactions_work_on_a_rope() {
    let mut document = rope_document("one two three");

    document
        .apply_edits(&[
            TextEdit::replace(TextRange::new(TextOffset(0), TextOffset(3)), "1"),
            TextEdit::replace(TextRange::new(TextOffset(4), TextOffset(7)), "2"),
            TextEdit::replace(TextRange::new(TextOffset(8), TextOffset(13)), "3"),
        ])
        .unwrap();

    assert_eq!(document.text(), "1 2 3");
    assert_eq!(document.revision().as_u64(), 1);
}

#[test]
fn a_failing_transaction_leaves_the_rope_untouched() {
    let mut document = rope_document("abc");

    assert_eq!(
        document.apply_edits(&[
            TextEdit::insert(TextOffset(0), "x"),
            TextEdit::insert(TextOffset(99), "y"),
        ]),
        Err(TextError::OffsetOutOfRange { offset: 99, len: 3 })
    );

    assert_eq!(document.text(), "abc");
    assert_eq!(document.revision().as_u64(), 0);
}

#[test]
fn a_rope_refuses_to_split_a_character() {
    let mut document = rope_document("你好");

    assert_eq!(
        document.insert(TextOffset(1), "x"),
        Err(TextError::NotCharBoundary { offset: 1 })
    );
    assert_eq!(document.text(), "你好");
}

#[test]
fn a_read_only_rope_document_refuses_edits() {
    let mut document = rope_document("abc");
    document.set_read_only(true);

    assert_eq!(
        document.insert(TextOffset(3), "!"),
        Err(TextError::ReadOnly)
    );
}

/// The two backends have to behave the same. The document level rules
/// (revision, transactions, errors) are backend independent, so the same
/// sequence of operations has to produce the same text on both.
#[test]
fn both_backends_produce_the_same_text() {
    let mut rope = rope_document("");
    let mut string = string_document("");

    let operations: [(TextOffset, &str); 4] = [
        (TextOffset(0), "Hello"),
        (TextOffset(5), " world"),
        (TextOffset(0), ">> "),
        (TextOffset(3), "中"),
    ];

    for (offset, text) in operations {
        rope.insert(offset, text).unwrap();
        string.insert(offset, text).unwrap();
    }

    assert_eq!(rope.text(), string.text());
    assert_eq!(rope.revision(), string.revision());

    // ">> " plus the three bytes of "中" end on a character boundary.
    let range = TextRange::new(TextOffset(0), TextOffset(6));

    assert_eq!(rope.text_in(range).unwrap(), ">> 中");
    assert_eq!(rope.text_in(range).unwrap(), string.text_in(range).unwrap());
}

#[test]
fn the_chunk_layout_is_visible_through_the_store() {
    let store = RopeTextStore::from_text(&"a".repeat(5000));

    assert!(store.rope().leaf_count() > 1);
    assert!(store.rope().longest_leaf() <= 1024);
}
