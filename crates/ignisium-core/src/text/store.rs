use std::borrow::Cow;

use super::{validate_offset, validate_range};
use crate::error::TextResult;
use crate::{TextOffset, TextRange};

/// Abstraction over the underlying text storage.
///
/// `TextStore` only describes the operations the editor core needs, so that
/// other backends (Rope, Piece Table, Gap Buffer) can be added without
/// changing `Document`.
///
/// All offsets are UTF-8 byte offsets (ADR-0001). Mutating operations are
/// fallible: invalid input becomes a `TextError` instead of a panic
/// (ADR-0003).
///
/// The trait reads text and ranges, never chunks: a contiguous backend answers
/// with a borrowed slice, a chunked backend with an owned string, and the
/// storage layout stays behind the boundary (ADR-0008).
pub trait TextStore {
    /// Length of the stored text in UTF-8 bytes.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The whole text.
    ///
    /// The result is a snapshot, not a view: a backend that stores the text in
    /// chunks has to materialise it. Callers that only need a part of the text
    /// should use [`TextStore::text_in`].
    fn text(&self) -> Cow<'_, str>;

    /// A range of the text, `[start, end)`.
    ///
    /// An invalid range is a `TextError`, never a panic.
    fn text_in(&self, range: TextRange) -> TextResult<Cow<'_, str>>;

    fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<()>;

    fn delete(&mut self, range: TextRange) -> TextResult<()>;

    /// Whether `offset` sits on a UTF-8 character boundary.
    ///
    /// An offset past the end of the text is not a boundary. Backends answer
    /// this without materialising the text, because every edit goes through
    /// it.
    fn is_char_boundary(&self, offset: TextOffset) -> bool;

    /// Rejects offsets that are past the end or inside a multi-byte character.
    ///
    /// Validation rules belong to the offset model, not to a storage
    /// implementation, so every backend shares this one.
    fn validate_offset(&self, offset: TextOffset) -> TextResult<()> {
        validate_offset(self.len(), self.is_char_boundary(offset), offset)
    }

    /// Rejects ranges whose endpoints are inverted or invalid.
    fn validate_range(&self, range: TextRange) -> TextResult<()> {
        validate_range(self.len(), range, |offset| self.is_char_boundary(offset))
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

    /// A store holding `text`.
    ///
    /// This is loading, not editing: a `&str` is valid UTF-8 by construction,
    /// so there is nothing to validate and nothing to fail.
    pub fn from_text(text: &str) -> Self {
        Self {
            text: text.to_string(),
        }
    }

    /// The text as a borrowed slice.
    ///
    /// Only a contiguous backend can offer this without copying, so it is an
    /// inherent method and not part of the trait.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl TextStore for StringTextStore {
    fn len(&self) -> usize {
        self.text.len()
    }

    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn text_in(&self, range: TextRange) -> TextResult<Cow<'_, str>> {
        self.validate_range(range)?;

        Ok(Cow::Borrowed(&self.text[range.start.0..range.end.0]))
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

    fn is_char_boundary(&self, offset: TextOffset) -> bool {
        // `str::is_char_boundary` panics for an index past the end, and this
        // method is public.
        offset.0 <= self.text.len() && self.text.is_char_boundary(offset.0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TextError;

    #[test]
    fn new_store_is_empty() {
        let store = StringTextStore::new();

        assert_eq!(store.as_str(), "");
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());
    }

    #[test]
    fn a_store_can_be_loaded_with_text() {
        let store = StringTextStore::from_text("你好");

        assert_eq!(store.as_str(), "你好");
        assert_eq!(store.len(), 6);
        assert!(matches!(store.text(), Cow::Borrowed(_)));
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
    fn an_offset_past_the_end_is_not_a_boundary() {
        let store = StringTextStore::new();

        // `str::is_char_boundary` panics for such an index, the store does not.
        assert!(!store.is_char_boundary(TextOffset(1)));
        assert!(!store.is_char_boundary(TextOffset(99)));
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

    #[test]
    fn the_whole_text_is_borrowed_by_a_contiguous_backend() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "Hello").unwrap();

        assert!(matches!(store.text(), Cow::Borrowed(_)));
    }

    #[test]
    fn a_range_of_the_text_is_borrowed_by_a_contiguous_backend() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "Hello").unwrap();

        let range = store
            .text_in(TextRange::new(TextOffset(1), TextOffset(4)))
            .unwrap();

        assert_eq!(range, "ell");
        assert!(matches!(range, Cow::Borrowed(_)));
    }

    #[test]
    fn an_invalid_range_cannot_be_read() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "Hello").unwrap();

        assert_eq!(
            store.text_in(TextRange::new(TextOffset(3), TextOffset(1))),
            Err(TextError::InvalidRange { start: 3, end: 1 })
        );
        assert_eq!(
            store.text_in(TextRange::new(TextOffset(0), TextOffset(99))),
            Err(TextError::OffsetOutOfRange { offset: 99, len: 5 })
        );
    }

    #[test]
    fn an_empty_range_is_empty_text() {
        let mut store = StringTextStore::new();

        store.insert(TextOffset(0), "Hello").unwrap();

        let range = store
            .text_in(TextRange::new(TextOffset(2), TextOffset(2)))
            .unwrap();

        assert_eq!(range, "");
    }

    // Trait object check: the abstraction is usable through `dyn TextStore`,
    // so a host can hold a backend without knowing its concrete type.
    // `Document` itself still uses the concrete type, see ADR-0002.
    #[test]
    fn edit_through_trait_object() {
        let mut store = StringTextStore::new();
        let store: &mut dyn TextStore = &mut store;

        store.insert(TextOffset(0), "你好").unwrap();

        assert_eq!(store.text(), "你好");
        assert_eq!(store.len(), 6);
    }

    #[test]
    fn read_through_trait_object() {
        let mut store = StringTextStore::new();
        store.insert(TextOffset(0), "Hello").unwrap();

        let store: &dyn TextStore = &store;

        assert_eq!(
            store
                .text_in(TextRange::new(TextOffset(0), TextOffset(2)))
                .unwrap(),
            "He"
        );
    }
}
