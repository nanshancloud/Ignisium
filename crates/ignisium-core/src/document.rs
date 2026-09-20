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

    // Known limitation, recorded in ADR-0001:
    // TextOffset is a UTF-8 byte offset, so an offset that lands inside a
    // multi-byte character is invalid. Validation is introduced in ADR-0003.
    #[test]
    #[should_panic]
    fn insert_inside_utf8_character_panics() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "你好").unwrap();
        document.insert(TextOffset(1), "x").unwrap();
    }
}
