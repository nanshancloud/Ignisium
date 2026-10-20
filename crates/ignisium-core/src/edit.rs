use std::fmt;

use crate::error::TextError;
use crate::{TextOffset, TextRange};

/// One edit inside a batch.
///
/// Every edit in a batch is expressed against **the text as it was before the
/// batch started**, not against the intermediate result of the previous edit.
/// That is what makes a batch expressible in one pass and what makes it
/// checkable before anything is modified. See ADR-0006.
///
/// ```text
/// text:      "  if x {"
/// batch:     Delete [0, 2)          indentation
///            Insert at 2, "\n  "    newline plus indentation
/// result:    "\n  if x {"
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextEdit {
    /// Insert `text` at `offset`.
    Insert { offset: TextOffset, text: String },

    /// Remove `range`.
    Delete { range: TextRange },

    /// Remove `range` and insert `text` at `range.start`.
    Replace { range: TextRange, text: String },
}

impl TextEdit {
    /// An edit that inserts `text` at `offset`.
    pub fn insert(offset: TextOffset, text: impl Into<String>) -> Self {
        TextEdit::Insert {
            offset,
            text: text.into(),
        }
    }

    /// An edit that removes `range`.
    pub fn delete(range: TextRange) -> Self {
        TextEdit::Delete { range }
    }

    /// An edit that replaces `range` with `text`.
    pub fn replace(range: TextRange, text: impl Into<String>) -> Self {
        TextEdit::Replace {
            range,
            text: text.into(),
        }
    }

    /// The part of the original text this edit claims.
    ///
    /// An insertion claims nothing, so its span is empty (`start == end`).
    pub fn span(&self) -> TextRange {
        match self {
            TextEdit::Insert { offset, .. } => TextRange::new(*offset, *offset),
            TextEdit::Delete { range } | TextEdit::Replace { range, .. } => *range,
        }
    }

    /// Whether applying this edit would leave the text unchanged.
    ///
    /// No-op edits are dropped from a batch before it is validated and applied:
    /// an edit with no effect has no error and no position to keep clear of the
    /// other edits.
    pub fn is_noop(&self) -> bool {
        match self {
            TextEdit::Insert { text, .. } => text.is_empty(),
            TextEdit::Delete { range } => range.is_empty(),
            TextEdit::Replace { range, text } => range.is_empty() && text.is_empty(),
        }
    }

    /// Whether this edit inserts text instead of consuming a range.
    pub(crate) fn is_insertion(&self) -> bool {
        matches!(self, TextEdit::Insert { .. })
    }

    /// The interval used to detect two edits that fight over the same text.
    ///
    /// Half-open ranges claim exactly what they span. An insertion claims
    /// nothing, but it still needs a position no other edit may consume, so it
    /// claims the single position `offset..offset + 1`.
    pub(crate) fn claim(&self) -> TextRange {
        match self {
            TextEdit::Insert { offset, .. } => TextRange::new(*offset, TextOffset(offset.0 + 1)),
            TextEdit::Delete { range } | TextEdit::Replace { range, .. } => *range,
        }
    }
}

/// A problem found in a batch edit, together with the offending edit.
///
/// `Document::edit_problems` reports every problem it can find, not only the
/// first one, so a host can highlight all the edits it has to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditProblem {
    /// Index of the edit in the batch it was passed as.
    pub index: usize,

    /// What is wrong with that edit.
    pub error: TextError,
}

impl fmt::Display for EditProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "edit {}: {}", self.index, self.error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_span_is_empty_at_its_offset() {
        let edit = TextEdit::insert(TextOffset(4), "x");

        assert_eq!(edit.span(), TextRange::new(TextOffset(4), TextOffset(4)));
        assert!(edit.is_insertion());
    }

    #[test]
    fn delete_span_is_its_range() {
        let range = TextRange::new(TextOffset(1), TextOffset(4));
        let edit = TextEdit::delete(range);

        assert_eq!(edit.span(), range);
        assert!(!edit.is_insertion());
    }

    #[test]
    fn replace_span_is_its_range() {
        let range = TextRange::new(TextOffset(1), TextOffset(4));
        let edit = TextEdit::replace(range, "x");

        assert_eq!(edit.span(), range);
        assert!(!edit.is_insertion());
    }

    #[test]
    fn edits_own_their_payload() {
        let edit = TextEdit::insert(TextOffset(0), String::from("hi"));

        assert_eq!(
            edit,
            TextEdit::Insert {
                offset: TextOffset(0),
                text: String::from("hi")
            }
        );
    }

    #[test]
    fn no_op_edits_are_recognized() {
        assert!(TextEdit::insert(TextOffset(0), "").is_noop());
        assert!(TextEdit::delete(TextRange::new(TextOffset(1), TextOffset(1))).is_noop());
        assert!(TextEdit::replace(TextRange::new(TextOffset(1), TextOffset(1)), "").is_noop());
    }

    #[test]
    fn edits_that_do_something_are_not_no_ops() {
        assert!(!TextEdit::insert(TextOffset(0), "x").is_noop());
        assert!(!TextEdit::delete(TextRange::new(TextOffset(0), TextOffset(1))).is_noop());
        assert!(!TextEdit::replace(TextRange::new(TextOffset(0), TextOffset(1)), "x").is_noop());
        // Replacing nothing with something is still an insertion.
        assert!(!TextEdit::replace(TextRange::new(TextOffset(0), TextOffset(0)), "x").is_noop());
    }

    #[test]
    fn an_insertion_claims_the_position_it_is_written_at() {
        assert_eq!(
            TextEdit::insert(TextOffset(7), "x").claim(),
            TextRange::new(TextOffset(7), TextOffset(8))
        );
    }

    #[test]
    fn problem_display_names_the_edit() {
        let problem = EditProblem {
            index: 2,
            error: TextError::NotCharBoundary { offset: 5 },
        };

        assert_eq!(
            problem.to_string(),
            "edit 2: offset 5 is not a UTF-8 character boundary"
        );
    }
}
