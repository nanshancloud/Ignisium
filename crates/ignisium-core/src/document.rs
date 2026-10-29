use std::borrow::Cow;

use crate::edit::{EditProblem, TextEdit};
use crate::error::{TextError, TextResult};
use crate::metadata::DocumentMetadata;
use crate::revision::RevisionId;
use crate::text::{StringTextStore, TextStore};
use crate::{TextOffset, TextRange};

/// A text document.
///
/// The document is three things (ADR-0005): the characters in a [`TextStore`],
/// the metadata that says how the host wants the document treated, and the
/// revision that says which state the characters are in.
///
/// Editing operations return `TextResult<RevisionId>`: invalid offsets are
/// reported as `TextError`, never as a panic (ADR-0003). Several edits can be
/// applied as one atomic transaction through [`Document::apply_edits`]
/// (ADR-0006).
///
/// The document is generic over its storage backend, so a second backend can
/// be used through the same API (ADR-0008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document<S: TextStore = StringTextStore> {
    store: S,
    metadata: DocumentMetadata,
    revision: RevisionId,
}

impl Default for Document<StringTextStore> {
    fn default() -> Self {
        Self {
            store: StringTextStore::new(),
            metadata: DocumentMetadata::default(),
            revision: RevisionId::INITIAL,
        }
    }
}

impl Document<StringTextStore> {
    /// Creates an empty, writable document at [`RevisionId::INITIAL`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a document holding `text`.
    ///
    /// The document starts at [`RevisionId::INITIAL`]: loading content is not
    /// an edit. Loading is also infallible, so it does not go through the
    /// fallible editing path at all.
    pub fn from_text(text: &str) -> Self {
        Self {
            store: StringTextStore::from_text(text),
            metadata: DocumentMetadata::default(),
            revision: RevisionId::INITIAL,
        }
    }
}

impl<S: TextStore> Document<S> {
    /// Creates a document on top of an existing store.
    ///
    /// The document starts at [`RevisionId::INITIAL`]: the content of the
    /// store is not an edit.
    pub fn from_store(store: S) -> Self {
        Self {
            store,
            metadata: DocumentMetadata::default(),
            revision: RevisionId::INITIAL,
        }
    }

    /// The whole text.
    ///
    /// A contiguous backend answers with a borrowed slice, a chunked backend
    /// has to materialise the text.
    pub fn text(&self) -> Cow<'_, str> {
        self.store.text()
    }

    /// A range of the text, `[start, end)`.
    ///
    /// An invalid range is a `TextError`, never a panic.
    pub fn text_in(&self, range: TextRange) -> TextResult<Cow<'_, str>> {
        self.store.text_in(range)
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

    /// The storage backend, read only.
    ///
    /// There is deliberately no `store_mut`: the revision would no longer
    /// describe the content.
    pub fn store(&self) -> &S {
        &self.store
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
    /// Metadata changes do not advance the revision.
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
}
impl<S: TextStore> Document<S> {
    /// Inserts `text` at `offset`.
    ///
    /// Returns the revision produced by the edit. This is one edit expressed
    /// as a one-edit transaction, so it follows the same rules as
    /// [`Document::apply_edits`]: an edit that changes nothing does not
    /// advance the revision.
    pub fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<RevisionId> {
        self.apply_edits(&[TextEdit::insert(offset, text)])
    }

    /// Deletes `range`.
    ///
    /// Returns the revision produced by the edit. Like [`Document::insert`],
    /// this is a one-edit transaction.
    pub fn delete(&mut self, range: TextRange) -> TextResult<RevisionId> {
        self.apply_edits(&[TextEdit::delete(range)])
    }

    /// Replaces `range` with `text` as one edit.
    ///
    /// Replacing with an empty string is a delete; replacing an empty range is
    /// an insertion.
    pub fn replace(&mut self, range: TextRange, text: &str) -> TextResult<RevisionId> {
        self.apply_edits(&[TextEdit::replace(range, text)])
    }

    /// Applies `edits` as one atomic transaction and returns the revision the
    /// transaction produced.
    ///
    /// All offsets in the batch are interpreted against the text as it was when
    /// `apply_edits` was called. The rules are:
    ///
    /// - **All or nothing.** If any edit is invalid, the text is left as it
    ///   was and the first problem is returned. Use
    ///   [`Document::edit_problems`] to see all of them.
    /// - **No overlap.** Two edits may not claim the same text. Combine two
    ///   edits on the same spot into a single [`TextEdit::replace`].
    /// - **One revision.** A transaction advances the revision exactly once.
    /// - **No work, no revision.** Edits that would change nothing are dropped
    ///   before anything is validated.
    ///
    /// Edits are applied from the end of the document towards the start, so an
    /// edit never moves the text another edit still has to touch.
    pub fn apply_edits(&mut self, edits: &[TextEdit]) -> TextResult<RevisionId> {
        self.guard_writable()?;

        if let Some(problem) = self.edit_problems(edits).first() {
            return Err(problem.error.clone());
        }

        let mut ordered: Vec<(usize, &TextEdit)> = edits
            .iter()
            .enumerate()
            .filter(|(_, edit)| !edit.is_noop())
            .collect();

        if ordered.is_empty() {
            return Ok(self.revision);
        }

        // Back to front, so that every edit still sees the coordinates it was
        // written with: higher offsets first, at the same offset ranges before
        // insertions, and at the same offset and kind in reverse batch order.
        ordered.sort_by(|(left_index, left), (right_index, right)| {
            right
                .span()
                .start
                .cmp(&left.span().start)
                .then_with(|| left.is_insertion().cmp(&right.is_insertion()))
                .then_with(|| right_index.cmp(left_index))
        });

        for (_, edit) in ordered {
            self.apply_edit(edit)?;
        }

        Ok(self.advance_revision())
    }

    /// Reports every problem with `edits`, without touching the text.
    ///
    /// The problems are returned in batch order. An empty result means the
    /// batch can be applied. Whether the document accepts edits at all
    /// (`read_only`) is answered by `apply_edits`.
    pub fn edit_problems(&self, edits: &[TextEdit]) -> Vec<EditProblem> {
        let effective: Vec<(usize, &TextEdit)> = edits
            .iter()
            .enumerate()
            .filter(|(_, edit)| !edit.is_noop())
            .collect();

        let mut problems = Vec::new();

        for (position, (index, edit)) in effective.iter().enumerate() {
            self.collect_range_problems(*index, edit, &mut problems);

            for (other_index, other) in effective.iter().take(position) {
                if edit.claim().intersects(&other.claim()) {
                    problems.push(EditProblem {
                        index: *index,
                        error: TextError::OverlappingEdits {
                            first: *other_index,
                            second: *index,
                        },
                    });
                }
            }
        }

        problems.sort_by_key(|problem| problem.index);

        problems
    }

    /// One edit, applied to a document whose text already passed validation.
    fn apply_edit(&mut self, edit: &TextEdit) -> TextResult<()> {
        match edit {
            TextEdit::Insert { offset, text } => self.store.insert(*offset, text)?,
            TextEdit::Delete { range } => self.store.delete(*range)?,
            TextEdit::Replace { range, text } => {
                // `range.start` stays a character boundary after the removal.
                self.store.delete(*range)?;
                self.store.insert(range.start, text)?;
            }
        }

        Ok(())
    }

    fn collect_range_problems(
        &self,
        index: usize,
        edit: &TextEdit,
        problems: &mut Vec<EditProblem>,
    ) {
        let range = edit.span();

        if range.start > range.end {
            problems.push(EditProblem {
                index,
                error: TextError::InvalidRange {
                    start: range.start.0,
                    end: range.end.0,
                },
            });

            return;
        }

        // An insertion has a single endpoint; checking it twice would report
        // the same problem twice.
        let endpoints = if edit.is_insertion() { 1 } else { 2 };

        for offset in [range.start, range.end].into_iter().take(endpoints) {
            if let Err(error) = self.store.validate_offset(offset) {
                problems.push(EditProblem { index, error });
            }
        }
    }

    /// Read-only is document policy, not offset policy (ADR-0003).
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

    // -- replace ----------------------------------------------------------

    #[test]
    fn replace_swaps_a_range_for_new_text() {
        let mut document = Document::from_text("Hello world");

        document
            .replace(TextRange::new(TextOffset(6), TextOffset(11)), "there")
            .unwrap();

        assert_eq!(document.text(), "Hello there");
    }

    #[test]
    fn replace_with_nothing_is_a_delete() {
        let mut document = Document::from_text("Hello");

        document
            .replace(TextRange::new(TextOffset(0), TextOffset(1)), "")
            .unwrap();

        assert_eq!(document.text(), "ello");
    }

    #[test]
    fn replacing_an_empty_range_is_an_insertion() {
        let mut document = Document::from_text("Hllo");

        document
            .replace(TextRange::new(TextOffset(1), TextOffset(1)), "e")
            .unwrap();

        assert_eq!(document.text(), "Hello");
    }

    #[test]
    fn replace_is_one_edit_and_one_revision() {
        let mut document = Document::from_text("abc");

        assert_eq!(
            document.replace(TextRange::new(TextOffset(0), TextOffset(1)), "x"),
            Ok(RevisionId::INITIAL.next())
        );
        assert_eq!(document.text(), "xbc");
    }

    // -- transactions -----------------------------------------------------

    #[test]
    fn a_transaction_advances_the_revision_once() {
        let mut document = Document::from_text("  if x {");

        let revision = document
            .apply_edits(&[
                TextEdit::delete(TextRange::new(TextOffset(0), TextOffset(2))),
                TextEdit::insert(TextOffset(2), "\n  "),
            ])
            .unwrap();

        assert_eq!(revision, RevisionId::INITIAL.next());
        assert_eq!(document.revision(), revision);
        assert_eq!(document.text(), "\n  if x {");
    }

    #[test]
    fn edits_are_written_against_the_text_before_the_batch() {
        let mut document = Document::from_text("one two three");

        document
            .apply_edits(&[
                TextEdit::replace(TextRange::new(TextOffset(0), TextOffset(3)), "1"),
                TextEdit::replace(TextRange::new(TextOffset(8), TextOffset(13)), "3"),
            ])
            .unwrap();

        assert_eq!(document.text(), "1 two 3");
    }

    #[test]
    fn touching_ranges_may_be_deleted_in_one_batch() {
        let mut document = Document::from_text("abcdef");

        document
            .apply_edits(&[
                TextEdit::delete(TextRange::new(TextOffset(0), TextOffset(2))),
                TextEdit::delete(TextRange::new(TextOffset(2), TextOffset(4))),
            ])
            .unwrap();

        assert_eq!(document.text(), "ef");
    }

    #[test]
    fn overlapping_ranges_are_rejected() {
        let mut document = Document::from_text("abcdef");

        assert_eq!(
            document.apply_edits(&[
                TextEdit::delete(TextRange::new(TextOffset(0), TextOffset(4))),
                TextEdit::delete(TextRange::new(TextOffset(2), TextOffset(6))),
            ]),
            Err(TextError::OverlappingEdits {
                first: 0,
                second: 1
            })
        );
        assert_eq!(document.text(), "abcdef");
        assert_eq!(document.revision(), RevisionId::INITIAL);
    }

    #[test]
    fn an_insertion_inside_a_deleted_range_is_rejected() {
        let mut document = Document::from_text("abcdef");

        assert_eq!(
            document.apply_edits(&[
                TextEdit::delete(TextRange::new(TextOffset(0), TextOffset(6))),
                TextEdit::insert(TextOffset(3), "x"),
            ]),
            Err(TextError::OverlappingEdits {
                first: 0,
                second: 1
            })
        );
        assert_eq!(document.text(), "abcdef");
    }

    #[test]
    fn a_failing_edit_rolls_the_whole_batch_back() {
        let mut document = Document::from_text("abc");

        assert_eq!(
            document.apply_edits(&[
                TextEdit::insert(TextOffset(0), "1"),
                TextEdit::insert(TextOffset(99), "2"),
                TextEdit::insert(TextOffset(2), "3"),
            ]),
            Err(TextError::OffsetOutOfRange { offset: 99, len: 3 })
        );

        assert_eq!(document.text(), "abc");
        assert_eq!(document.revision(), RevisionId::INITIAL);
    }

    #[test]
    fn an_empty_batch_changes_nothing() {
        let mut document = Document::from_text("abc");

        assert_eq!(document.apply_edits(&[]), Ok(RevisionId::INITIAL));
        assert_eq!(document.text(), "abc");
        assert_eq!(document.revision(), RevisionId::INITIAL);
    }

    #[test]
    fn a_batch_of_no_ops_changes_nothing() {
        let mut document = Document::from_text("abc");

        assert_eq!(
            document.apply_edits(&[
                TextEdit::insert(TextOffset(1), ""),
                TextEdit::delete(TextRange::new(TextOffset(1), TextOffset(1))),
            ]),
            Ok(RevisionId::INITIAL)
        );

        assert_eq!(document.text(), "abc");
        assert_eq!(document.revision(), RevisionId::INITIAL);
    }

    #[test]
    fn no_op_edits_are_dropped_before_validation() {
        let mut document = Document::from_text("abc");

        // Even an out-of-range offset is ignored: an edit that would change
        // nothing has no effect and therefore nothing to complain about.
        assert_eq!(
            document.apply_edits(&[TextEdit::insert(TextOffset(99), "")]),
            Ok(RevisionId::INITIAL)
        );
        assert_eq!(document.text(), "abc");
    }

    #[test]
    fn an_insertion_at_the_start_of_a_deleted_range_is_rejected() {
        let mut document = Document::from_text("  if x {");

        // Both edits want the byte at offset 2, so the host has to write this
        // as a single `replace` instead.
        assert_eq!(
            document.apply_edits(&[
                TextEdit::delete(TextRange::new(TextOffset(2), TextOffset(3))),
                TextEdit::insert(TextOffset(2), "x"),
            ]),
            Err(TextError::OverlappingEdits {
                first: 0,
                second: 1
            })
        );
        assert_eq!(document.text(), "  if x {");
    }

    #[test]
    fn transactions_work_on_unicode_text() {
        let mut document = Document::from_text("你好世界");

        document
            .apply_edits(&[
                TextEdit::replace(TextRange::new(TextOffset(0), TextOffset(3)), "您"),
                TextEdit::insert(TextOffset(12), "!"),
            ])
            .unwrap();

        assert_eq!(document.text(), "您好世界!");
    }

    #[test]
    fn a_batch_never_splits_a_character() {
        let mut document = Document::from_text("你好");

        assert_eq!(
            document.apply_edits(&[TextEdit::delete(TextRange::new(
                TextOffset(1),
                TextOffset(3)
            ))]),
            Err(TextError::NotCharBoundary { offset: 1 })
        );
        assert_eq!(document.text(), "你好");
    }

    #[test]
    fn read_only_documents_refuse_transactions() {
        let mut document = Document::from_text("abc");
        document.set_read_only(true);

        assert_eq!(
            document.apply_edits(&[TextEdit::insert(TextOffset(0), "x")]),
            Err(TextError::ReadOnly)
        );
        assert_eq!(document.text(), "abc");
    }

    // -- edit problems ----------------------------------------------------

    #[test]
    fn every_problem_is_reported_in_batch_order() {
        let document = Document::from_text("abc");

        let problems = document.edit_problems(&[
            TextEdit::insert(TextOffset(99), "x"),
            TextEdit::delete(TextRange::new(TextOffset(0), TextOffset(0))),
            TextEdit::delete(TextRange::new(TextOffset(3), TextOffset(1))),
        ]);

        assert_eq!(
            problems,
            vec![
                EditProblem {
                    index: 0,
                    error: TextError::OffsetOutOfRange { offset: 99, len: 3 },
                },
                EditProblem {
                    index: 2,
                    error: TextError::InvalidRange { start: 3, end: 1 },
                },
            ]
        );
    }

    #[test]
    fn both_endpoints_of_a_range_are_checked() {
        let document = Document::from_text("abc");

        let problems = document.edit_problems(&[TextEdit::delete(TextRange::new(
            TextOffset(9),
            TextOffset(12),
        ))]);

        assert_eq!(problems.len(), 2);
        assert_eq!(
            problems[0].error,
            TextError::OffsetOutOfRange { offset: 9, len: 3 }
        );
        assert_eq!(
            problems[1].error,
            TextError::OffsetOutOfRange { offset: 12, len: 3 }
        );
    }

    #[test]
    fn overlapping_edits_are_reported_once_per_pair() {
        let document = Document::from_text("abcdef");

        let problems = document.edit_problems(&[
            TextEdit::delete(TextRange::new(TextOffset(0), TextOffset(4))),
            TextEdit::delete(TextRange::new(TextOffset(2), TextOffset(5))),
            TextEdit::delete(TextRange::new(TextOffset(1), TextOffset(3))),
        ]);

        assert_eq!(
            problems,
            vec![
                EditProblem {
                    index: 1,
                    error: TextError::OverlappingEdits {
                        first: 0,
                        second: 1
                    },
                },
                EditProblem {
                    index: 2,
                    error: TextError::OverlappingEdits {
                        first: 0,
                        second: 2
                    },
                },
                EditProblem {
                    index: 2,
                    error: TextError::OverlappingEdits {
                        first: 1,
                        second: 2
                    },
                },
            ]
        );
    }

    #[test]
    fn a_valid_batch_has_no_problems() {
        let document = Document::from_text("abc");

        assert!(
            document
                .edit_problems(&[TextEdit::insert(TextOffset(3), "x")])
                .is_empty()
        );
    }

    // -- generic backend --------------------------------------------------

    #[test]
    fn a_document_can_be_built_on_a_store() {
        let mut store = StringTextStore::new();
        store.insert(TextOffset(0), "Hello").unwrap();

        let mut document = Document::from_store(store);

        assert_eq!(document.text(), "Hello");
        assert_eq!(document.revision(), RevisionId::INITIAL);

        document.insert(TextOffset(5), "!").unwrap();
        assert_eq!(document.text(), "Hello!");
    }

    #[test]
    fn a_range_of_the_text_can_be_read() {
        let document = Document::from_text("Hello world");

        assert_eq!(
            document
                .text_in(TextRange::new(TextOffset(0), TextOffset(5)))
                .unwrap(),
            "Hello"
        );
        assert_eq!(
            document
                .text_in(TextRange::new(TextOffset(6), TextOffset(11)))
                .unwrap(),
            "world"
        );
    }

    #[test]
    fn an_invalid_range_cannot_be_read() {
        let document = Document::from_text("Hello");

        assert_eq!(
            document.text_in(TextRange::new(TextOffset(0), TextOffset(99))),
            Err(TextError::OffsetOutOfRange { offset: 99, len: 5 })
        );
        assert_eq!(
            document.text_in(TextRange::new(TextOffset(3), TextOffset(1))),
            Err(TextError::InvalidRange { start: 3, end: 1 })
        );
    }

    #[test]
    fn the_whole_text_of_a_contiguous_backend_is_borrowed() {
        let document = Document::from_text("Hello");

        assert!(matches!(document.text(), std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn the_store_is_reachable_for_reading() {
        let document = Document::from_text("Hello");

        assert_eq!(document.store().len(), 5);
    }

    // -- one edit is one transaction ---------------------------------------

    #[test]
    fn an_empty_insert_does_not_advance_the_revision() {
        let mut document = Document::from_text("abc");

        assert_eq!(document.insert(TextOffset(1), ""), Ok(RevisionId::INITIAL));
        assert_eq!(document.text(), "abc");
        assert_eq!(document.revision(), RevisionId::INITIAL);
    }

    #[test]
    fn an_empty_delete_does_not_advance_the_revision() {
        let mut document = Document::from_text("abc");

        assert_eq!(
            document.delete(TextRange::new(TextOffset(1), TextOffset(1))),
            Ok(RevisionId::INITIAL)
        );
        assert_eq!(document.text(), "abc");
        assert_eq!(document.revision(), RevisionId::INITIAL);
    }

    /// The single edit entry points and the transaction entry point are the
    /// same engine, so they cannot disagree about what an edit did.
    #[test]
    fn one_edit_and_a_one_edit_transaction_do_the_same_thing() {
        let mut direct = Document::from_text("abc");
        let mut batch = Document::from_text("abc");

        let from_insert = direct.insert(TextOffset(1), "x").unwrap();
        let from_batch = batch
            .apply_edits(&[TextEdit::insert(TextOffset(1), "x")])
            .unwrap();

        assert_eq!(from_insert, from_batch);
        assert_eq!(direct.text(), batch.text());
        assert_eq!(direct.revision(), batch.revision());

        let from_delete = direct
            .delete(TextRange::new(TextOffset(0), TextOffset(1)))
            .unwrap();
        let from_batch = batch
            .apply_edits(&[TextEdit::delete(TextRange::new(
                TextOffset(0),
                TextOffset(1),
            ))])
            .unwrap();

        assert_eq!(from_delete, from_batch);
        assert_eq!(direct.text(), batch.text());
    }

    #[test]
    fn loading_a_document_is_not_an_edit() {
        let document = Document::from_text("Hello");

        assert_eq!(document.revision(), RevisionId::INITIAL);
        assert_eq!(document.text(), "Hello");
    }
}
