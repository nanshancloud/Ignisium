use crate::text::{StringTextStore, TextStore};
use crate::{TextOffset, TextRange};

/// A text document.
///
/// `Document` does not touch `String` directly. It talks to the storage layer
/// through `TextStore` so the backend can be replaced later.
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

    pub fn insert(&mut self, offset: TextOffset, text: &str) {
        self.store.insert(offset, text);
    }

    pub fn delete(&mut self, range: TextRange) {
        self.store.delete(range);
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

        document.insert(TextOffset(0), "Hello");

        assert_eq!(document.text(), "Hello");
    }

    #[test]
    fn insert_middle() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "Hllo");
        document.insert(TextOffset(1), "e");

        assert_eq!(document.text(), "Hello");
    }

    #[test]
    fn delete_text() {
        let mut document = Document::new();

        document.insert(TextOffset(0), "Hello");

        document.delete(TextRange::new(TextOffset(1), TextOffset(4)));

        assert_eq!(document.text(), "Ho");
    }
}
