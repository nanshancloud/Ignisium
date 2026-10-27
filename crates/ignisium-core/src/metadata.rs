/// Document state that is not part of the text itself.
///
/// Deliberately small: a field is only added here when a caller needs it.
/// Currently `read_only`: the host refuses edits through this document.
///
/// Not here yet: newline and encoding policy (belongs with the line index of
/// week 13), file name, path and dirty flag (application level).
///
/// See ADR-0005.
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
    /// Toggling this flag is not an edit: it does not advance the revision.
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
