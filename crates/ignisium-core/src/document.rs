use crate::error::TextResult;
use crate::text::{StringTextStore, TextStore};
use crate::{TextOffset, TextRange};

/// A text document.
///
/// `Document` does not touch `String` directly. It talks to the storage layer
/// through `TextStore` so the backend can be replaced later.
///
/// Editing operations return `TextResult<()>`: invalid offsets are reported as
/// `TextError`, never as a panic. See ADR-0003.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    store: StringTextStore,
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        self.store.as_str()
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    /// Whether `offset` sits on a UTF-8 character boundary.
    pub fn is_char_boundary(&self, offset: TextOffset) -> bool {
        self.store.is_char_boundary(offset)
    }

    pub fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<()> {
        self.store.insert(offset, text)
    }

    pub fn delete(&mut self, range: TextRange) -> TextResult<()> {
        self.store.delete(range)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TextError;

    #[test]
    fn create_empty_document() {
        let document = Document::new();

        assert_eq!(document.text(), "");
        assert!(document.is_empty());
    }

    #[test]
    fn insert_text() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "Hello").unwrap();

        assert_eq!(document.text(), "Hello");
    }

    #[test]
    fn insert_middle() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "Hllo").unwrap();
        document.insert(TextOffset(1), "e").unwrap();

        assert_eq!(document.text(), "Hello");
    }

    #[test]
    fn delete_text() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "Hello").unwrap();

        document
            .delete(TextRange::new(TextOffset(1), TextOffset(4)))
            .unwrap();

        assert_eq!(document.text(), "Ho");
    }

    #[test]
    fn unicode_text() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "你好").unwrap();

        assert_eq!(document.text(), "你好");
    }

    #[test]
    fn unicode_append() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "你好").unwrap();
        document.insert(TextOffset(6), "世界").unwrap();

        assert_eq!(document.text(), "你好世界");
    }

    #[test]
    fn unicode_len_is_byte_length() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "你好").unwrap();

        assert_eq!(document.text().chars().count(), 2);
        assert_eq!(document.len(), 6);
    }

    #[test]
    fn delete_unicode_char_by_byte_range() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "你好").unwrap();
        document
            .delete(TextRange::new(TextOffset(0), TextOffset(3)))
            .unwrap();

        assert_eq!(document.text(), "好");
    }

    #[test]
    fn char_boundary_reflects_utf8_layout() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "你好").unwrap();

        assert!(document.is_char_boundary(TextOffset(0)));
        assert!(document.is_char_boundary(TextOffset(3)));
        assert!(document.is_char_boundary(TextOffset(6)));
        assert!(!document.is_char_boundary(TextOffset(1)));
        assert!(!document.is_char_boundary(TextOffset(2)));
    }

    // Replaces the `#[should_panic]` test from week 2: invalid input is now a
    // value the caller can handle, and the document is left untouched.
    #[test]
    fn insert_inside_utf8_character_returns_error() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "你好").unwrap();

        assert_eq!(
            document.insert(TextOffset(1), "x"),
            Err(TextError::NotCharBoundary { offset: 1 })
        );
        assert_eq!(document.text(), "你好");
    }

    #[test]
    fn insert_after_end_returns_error() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "Hi").unwrap();

        assert_eq!(
            document.insert(TextOffset(9), "!"),
            Err(TextError::OffsetOutOfRange { offset: 9, len: 2 })
        );
        assert_eq!(document.text(), "Hi");
    }

    #[test]
    fn delete_inverted_range_returns_error() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "Hello").unwrap();

        assert_eq!(
            document.delete(TextRange::new(TextOffset(3), TextOffset(1))),
            Err(TextError::InvalidRange { start: 3, end: 1 })
        );
        assert_eq!(document.text(), "Hello");
    }
}
