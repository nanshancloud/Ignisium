use crate::error::{TextError, TextResult};
use crate::{TextOffset, TextRange};

/// Abstraction over the underlying text storage.
///
/// `TextStore` is intentionally small. It only describes the operations the
/// editor core needs today, so future backends (Rope, Piece Table, Gap Buffer,
/// Chunked storage) can be added without changing `Document`.
///
/// All offsets are UTF-8 byte offsets. See ADR-0001.
///
/// v0.2: mutating operations are fallible. Offsets are validated before the
/// backend is touched, so invalid input becomes a `TextError` instead of a
/// panic. See ADR-0003.
pub trait TextStore {
    /// Length of the stored text in UTF-8 bytes.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whole text as a single string slice.
    fn as_str(&self) -> &str;

    fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<()>;

    fn delete(&mut self, range: TextRange) -> TextResult<()>;

    /// Whether `offset` sits on a UTF-8 character boundary.
    ///
    /// The default implementation goes through `as_str()`, which is O(1) for a
    /// contiguous backend but may be expensive for a chunked one. Backends that
    /// can answer cheaper should override it.
    fn is_char_boundary(&self, offset: TextOffset) -> bool {
        self.as_str().is_char_boundary(offset.0)
    }

    /// Reject offsets that are past the end or inside a multi-byte character.
    ///
    /// Shared by every backend: validation rules belong to the offset model,
    /// not to a specific storage implementation.
    fn validate_offset(&self, offset: TextOffset) -> TextResult<()> {
        let len = self.len();

        if offset.0 > len {
            return Err(TextError::OffsetOutOfRange {
                offset: offset.0,
                len,
            });
        }

        if !self.is_char_boundary(offset) {
            return Err(TextError::NotCharBoundary { offset: offset.0 });
        }

        Ok(())
    }

    /// Reject ranges whose endpoints are inverted or invalid.
    fn validate_range(&self, range: TextRange) -> TextResult<()> {
        if range.start.0 > range.end.0 {
            return Err(TextError::InvalidRange {
                start: range.start.0,
                end: range.end.0,
            });
        }

        self.validate_offset(range.start)?;
        self.validate_offset(range.end)?;

        Ok(())
    }
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

    fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<()> {
        self.validate_offset(offset)?;
        self.text.insert_str(offset.0, text);
        Ok(())
    }

    fn delete(&mut self, range: TextRange) -> TextResult<()> {
        self.validate_range(range)?;
        self.text.replace_range(range.start.0..range.end.0, "");
        Ok(())
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

        store.insert(TextOffset(0), "Hello").unwrap();

        assert_eq!(store.as_str(), "Hello");
        assert_eq!(store.len(), 5);
    }

    #[test]
    fn delete_text() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "Hello").unwrap();
        store
            .delete(TextRange::new(TextOffset(1), TextOffset(4)))
            .unwrap();

        assert_eq!(store.as_str(), "Ho");
    }

    #[test]
    fn len_counts_utf8_bytes_not_chars() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "你好").unwrap();

        assert_eq!(store.as_str().chars().count(), 2);
        assert_eq!(store.len(), 6);
    }

    #[test]
    fn char_boundaries_of_unicode_text() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "你好").unwrap();

        assert!(store.is_char_boundary(TextOffset(0)));
        assert!(store.is_char_boundary(TextOffset(3)));
        assert!(store.is_char_boundary(TextOffset(6)));
        assert!(!store.is_char_boundary(TextOffset(1)));
    }

    #[test]
    fn insert_out_of_range_is_an_error() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "Hi").unwrap();

        assert_eq!(
            store.insert(TextOffset(9), "!"),
            Err(TextError::OffsetOutOfRange { offset: 9, len: 2 })
        );
        assert_eq!(store.as_str(), "Hi");
    }

    #[test]
    fn insert_inside_character_is_an_error() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "你好").unwrap();

        assert_eq!(
            store.insert(TextOffset(1), "x"),
            Err(TextError::NotCharBoundary { offset: 1 })
        );
        assert_eq!(store.as_str(), "你好");
    }

    #[test]
    fn delete_inverted_range_is_an_error() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "Hello").unwrap();

        assert_eq!(
            store.delete(TextRange::new(TextOffset(3), TextOffset(1))),
            Err(TextError::InvalidRange { start: 3, end: 1 })
        );
        assert_eq!(store.as_str(), "Hello");
    }

    // Trait object check: the abstraction is usable through `dyn TextStore`,
    // so a host can hold a backend without knowing its concrete type.
    // `Document` itself still uses the concrete type, see ADR-0002.
    #[test]
    fn edit_through_trait_object() {
        let mut store = StringTextStore::new();
        let store: &mut dyn TextStore = &mut store;

        store.insert(TextOffset(0), "你好").unwrap();

        assert_eq!(store.as_str(), "你好");
        assert_eq!(store.len(), 6);
    }
}
