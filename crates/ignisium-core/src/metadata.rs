/// Document state that is not part of the text itself.
///
/// The text storage layer answers "what are the characters?". This type answers
/// the few "what kind of document is this?" questions the editing core already
/// has a use case for. It is deliberately small: a field is only added here when
/// a caller needs it, so the document state does not accumulate speculative
/// options that no host can act on.
///
/// Current fields:
///
/// - `read_only`: the host refuses edits through this document. Needed for
///   read-only views and for documents opened as generated output.
///
/// Deliberately not here yet:
///
/// - newline / encoding policy: belongs with the line index (week 13), and
///   putting it in the document now would make the storage layer depend on it
/// - file name, path, dirty flag: application level (IgnisTE), not core
///
/// See ADR-0005 for the lifecycle of this type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DocumentMetadata {
    read_only: bool,
}

impl DocumentMetadata {
    /// Whether edits are refused on the owning document.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Sets whether edits are refused on the owning document.
    ///
    /// Toggling this flag is not an edit: it does not change the text and does
    /// not advance the document revision.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_metadata_is_writable() {
        let metadata = DocumentMetadata::default();

        assert!(!metadata.is_read_only());
    }

    #[test]
    fn read_only_can_be_toggled() {
        let mut metadata = DocumentMetadata::default();

        metadata.set_read_only(true);
        assert!(metadata.is_read_only());

        metadata.set_read_only(false);
        assert!(!metadata.is_read_only());
    }
}
