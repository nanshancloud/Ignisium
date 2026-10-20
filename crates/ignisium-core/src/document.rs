use crate::error::{TextError, TextResult};
use crate::metadata::DocumentMetadata;
use crate::revision::RevisionId;
use crate::text::{StringTextStore, TextStore};
use crate::{TextOffset, TextRange};

/// A text document.
///
/// `Document` does not touch `String` directly. It talks to the storage layer
/// through `TextStore` so the backend can be replaced later.
///
/// Since v0.3 a document is three things (ADR-0005):
///
/// ```text
/// Document
///    │
///    ├── store     StringTextStore   the characters
///    ├── metadata  DocumentMetadata  how the host wants it treated
///    └── revision  RevisionId        which state the characters are in
/// ```
///
/// Editing operations return `TextResult<RevisionId>`: invalid offsets are
/// reported as `TextError`, never as a panic (ADR-0003), and a successful edit
/// reports the revision it produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    store: StringTextStore,
    metadata: DocumentMetadata,
    revision: RevisionId,
}

impl Document {
    /// Creates an empty, writable document at [`RevisionId::INITIAL`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a document holding `text`.
    ///
    /// The document starts at [`RevisionId::INITIAL`]: loading content is not an
    /// edit, so consumers may treat revision 0 as "nothing cached yet".
    pub fn from_text(text: &str) -> Self {
        let mut document = Self::new();

        // `text` is a whole `&str`, so offset 0 of an empty document is always
        // a valid insertion point and this cannot fail.
        document
            .store
            .insert(TextOffset(0), text)
            .expect("inserting at offset 0 into an empty document cannot fail");

        document
    }

    /// The whole text as one string slice.
    pub fn text(&self) -> &str {
        self.store.as_str()
    }

    /// Length of the text in UTF-8 bytes.
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

    /// The revision of the current text.
    pub fn revision(&self) -> RevisionId {
        self.revision
    }

    /// Host-visible document metadata.
    pub fn metadata(&self) -> &DocumentMetadata {
        &self.metadata
    }

    /// Mutable access to the metadata, for example to toggle `read_only`.
    ///
    /// Changing metadata does not change the text, so it does not advance the
    /// revision.
    pub fn metadata_mut(&mut self) -> &mut DocumentMetadata {
        &mut self.metadata
    }

    /// Refuses or allows edits through this document.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.metadata.set_read_only(read_only);
    }

    /// Whether edits are refused on this document.
    pub fn is_read_only(&self) -> bool {
        self.metadata.is_read_only()
    }

    /// Inserts `text` at `offset`.
    ///
    /// Returns the revision produced by the edit.
    pub fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<RevisionId> {
        self.guard_writable()?;
        self.store.insert(offset, text)?;
        Ok(self.advance_revision())
    }

    /// Deletes `range`.
    ///
    /// Returns the revision produced by the edit.
    pub fn delete(&mut self, range: TextRange) -> TextResult<RevisionId> {
        self.guard_writable()?;
        self.store.delete(range)?;
        Ok(self.advance_revision())
    }

    /// Read-only is document policy, not offset policy.
    ///
    /// Offset and range validation belongs to the storage layer (ADR-0003); this
    /// check belongs to the document because the store knows nothing about how
    /// the host wants to treat the document.
    fn guard_writable(&self) -> TextResult<()> {
        if self.metadata.is_read_only() {
            return Err(TextError::ReadOnly);
        }

        Ok(())
    }

    fn advance_revision(&mut self) -> RevisionId {
        self.revision = self.revision.next();

        self.revision
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
        assert_eq!(document.len(), 0);
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

    // -- revision ---------------------------------------------------------

    #[test]
    fn new_document_is_at_the_initial_revision() {
        assert_eq!(Document::new().revision(), RevisionId::INITIAL);
    }

    #[test]
    fn from_text_stays_at_the_initial_revision() {
        let document = Document::from_text("Hello");

        assert_eq!(document.text(), "Hello");
        assert_eq!(document.revision(), RevisionId::INITIAL);
    }

    #[test]
    fn every_successful_edit_advances_the_revision_once() {
        let mut document = Document::new();
        let first = RevisionId::INITIAL.next();
        let second = first.next();
        let third = second.next();

        assert_eq!(document.insert(TextOffset(0), "Hello"), Ok(first));
        assert_eq!(document.insert(TextOffset(5), "!"), Ok(second));
        assert_eq!(
            document.delete(TextRange::new(TextOffset(0), TextOffset(1))),
            Ok(third)
        );

        assert_eq!(document.revision(), third);
        assert_eq!(document.text(), "ello!");
    }

    #[test]
    fn failed_edits_do_not_advance_the_revision() {
        let mut document = Document::from_text("你好");

        assert!(document.insert(TextOffset(1), "x").is_err());
        assert!(
            document
                .delete(TextRange::new(TextOffset(3), TextOffset(0)))
                .is_err()
        );
        assert!(document.insert(TextOffset(99), "x").is_err());

        assert_eq!(document.revision(), RevisionId::INITIAL);
        assert_eq!(document.text(), "你好");
    }

    #[test]
    fn cloned_documents_keep_their_revision() {
        let mut document = Document::new();
        document.insert(TextOffset(0), "Hello").unwrap();

        let copy = document.clone();

        assert_eq!(copy.revision(), document.revision());
        assert_eq!(copy, document);
    }

    // -- metadata ---------------------------------------------------------

    #[test]
    fn new_document_is_writable() {
        assert!(!Document::new().is_read_only());
    }

    #[test]
    fn read_only_document_refuses_edits() {
        let mut document = Document::from_text("Hello");
        document.set_read_only(true);

        assert_eq!(
            document.insert(TextOffset(5), "!"),
            Err(TextError::ReadOnly)
        );
        assert_eq!(
            document.delete(TextRange::new(TextOffset(0), TextOffset(1))),
            Err(TextError::ReadOnly)
        );

        assert_eq!(document.text(), "Hello");
        assert_eq!(document.revision(), RevisionId::INITIAL);
    }

    #[test]
    fn read_only_can_be_lifted() {
        let mut document = Document::from_text("Hello");
        document.set_read_only(true);
        document.set_read_only(false);

        assert_eq!(
            document.insert(TextOffset(5), "!"),
            Ok(RevisionId::INITIAL.next())
        );
        assert_eq!(document.text(), "Hello!");
    }

    #[test]
    fn metadata_is_visible_through_the_document() {
        let mut document = Document::new();

        document.metadata_mut().set_read_only(true);

        assert!(document.metadata().is_read_only());
        assert!(document.is_read_only());
    }

    #[test]
    fn metadata_changes_do_not_advance_the_revision() {
        let mut document = Document::new();
        document.insert(TextOffset(0), "Hello").unwrap();

        document.set_read_only(true);
        document.set_read_only(false);

        assert_eq!(document.revision(), RevisionId::INITIAL.next());
    }
}
