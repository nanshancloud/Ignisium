use crate::{TextOffset, TextRange};

/// Abstraction over the underlying text storage.
///
/// `TextStore` is intentionally small. It only describes the operations the
/// editor core needs today, so future backends (Rope, Piece Table, Gap Buffer,
/// Chunked storage) can be added without changing `Document`.
///
/// All offsets are UTF-8 byte offsets. See ADR-0001.
pub trait TextStore {
    /// Length of the stored text in UTF-8 bytes.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whole text as a single string slice.
    fn as_str(&self) -> &str;

    fn insert(&mut self, offset: TextOffset, text: &str);

    fn delete(&mut self, range: TextRange);
}

/// Initial `TextStore` implementation backed by a contiguous `String`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StringTextStore {
    text: String,
}

impl StringTextStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TextStore for StringTextStore {
    fn len(&self) -> usize {
        self.text.len()
    }

    fn as_str(&self) -> &str {
        &self.text
    }

    fn insert(&mut self, offset: TextOffset, text: &str) {
        self.text.insert_str(offset.0, text);
    }

    fn delete(&mut self, range: TextRange) {
        self.text.replace_range(range.start.0..range.end.0, "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_store_is_empty() {
        let store = StringTextStore::new();

        assert_eq!(store.as_str(), "");
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());
    }

    #[test]
    fn insert_text() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "Hello");

        assert_eq!(store.as_str(), "Hello");
        assert_eq!(store.len(), 5);
    }

    #[test]
    fn delete_text() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "Hello");
        store.delete(TextRange::new(TextOffset(1), TextOffset(4)));

        assert_eq!(store.as_str(), "Ho");
    }

    #[test]
    fn len_counts_utf8_bytes_not_chars() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "你好");

        assert_eq!(store.as_str().chars().count(), 2);
        assert_eq!(store.len(), 6);
    }
}
