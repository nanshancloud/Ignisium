use std::fmt;

use crate::error::TextError;
use crate::offset_map::OffsetMap;
use crate::revision::RevisionId;
use crate::{TextOffset, TextRange};

/// A batch of edits, together with the revision it was written against.
///
/// The revision is what tells a batch that arrives late from a batch that
/// arrives on time: its offsets describe the text of revision `at`, so a
/// document that has moved on cannot apply it (ADR-0013 decision 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditBatch {
    /// The revision the offsets in `edits` were computed against.
    pub at: RevisionId,

    /// The edits, all expressed against the text of revision `at`.
    pub edits: Vec<TextEdit>,
}

impl EditBatch {
    /// A batch of `edits` written against revision `at`.
    pub fn new(at: RevisionId, edits: Vec<TextEdit>) -> Self {
        Self { at, edits }
    }
}

/// What a finished batch produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedEdits {
    /// The revision the document is at now.
    pub revision: RevisionId,

    /// Where the offsets of the text before the batch point now.
    pub map: OffsetMap,
}

/// One edit inside a batch.
///
/// Every edit in a batch is expressed against **the text as it was before the
/// batch started**, not against the intermediate result of the previous edit.
/// See ADR-0006.
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
    /// No-op edits are dropped from a batch before it is validated: an edit
    /// with no effect has no error and no position to keep clear.
    pub fn is_noop(&self) -> bool {
        match self {
            TextEdit::Insert { text, .. } => text.is_empty(),
            TextEdit::Delete { range } => range.is_empty(),
            TextEdit::Replace { range, text } => range.is_empty() && text.is_empty(),
        }
    }
}

/// The canonical form of one edit of a batch, borrowed from the caller.
///
/// `TextEdit` has several spellings for the same intent (`Replace` of an empty
/// range is an `Insert`, `Replace` with empty text is a `Delete`), and a host
/// may build the variants directly. Everything downstream therefore works on
/// this view, where one intent has one spelling (ADR-0012 decision 1).
pub(crate) struct Effective<'a> {
    index: usize,
    kind: EffectiveKind<'a>,
}

#[derive(Clone, Copy)]
pub(crate) enum EffectiveKind<'a> {
    Insert { offset: TextOffset, text: &'a str },
    Delete { range: TextRange },
    Replace { range: TextRange, text: &'a str },
}

impl<'a> Effective<'a> {
    /// The edit in its canonical form, for the code that applies it.
    pub(crate) fn kind(&self) -> EffectiveKind<'_> {
        self.kind
    }

    /// Index of this edit in the caller's batch, not in the normalized list.
    pub(crate) fn index(&self) -> usize {
        self.index
    }

    /// The part of the original text this edit occupies.
    ///
    /// An insertion occupies nothing, so its span is empty.
    pub(crate) fn span(&self) -> TextRange {
        match &self.kind {
            EffectiveKind::Insert { offset, .. } => TextRange::new(*offset, *offset),
            EffectiveKind::Delete { range } | EffectiveKind::Replace { range, .. } => *range,
        }
    }

    /// Whether this edit inserts text instead of consuming a range.
    pub(crate) fn is_insertion(&self) -> bool {
        matches!(&self.kind, EffectiveKind::Insert { .. })
    }

    /// The interval used to detect two edits that fight over the same text.
    ///
    /// An insertion claims nothing, but it still needs a position no other
    /// edit may consume, so it claims `offset..offset + 1`. The `+ 1` is a
    /// sentinel for comparison only, never a slice index, so it saturates: at
    /// `usize::MAX` the claim degenerates to an empty interval, which is
    /// harmless because such an offset is out of range for every real text and
    /// already reported as an error (ADR-0012 decision 2).
    pub(crate) fn claim(&self) -> TextRange {
        match &self.kind {
            EffectiveKind::Insert { offset, .. } => {
                TextRange::new(*offset, TextOffset(offset.0.saturating_add(1)))
            }
            EffectiveKind::Delete { range } | EffectiveKind::Replace { range, .. } => *range,
        }
    }

    /// Length in bytes of the text this edit puts in place of its span.
    pub(crate) fn replacement_len(&self) -> usize {
        match &self.kind {
            EffectiveKind::Insert { text, .. } | EffectiveKind::Replace { text, .. } => text.len(),
            EffectiveKind::Delete { .. } => 0,
        }
    }
}

/// Drops the edits that change nothing and gives the rest their canonical form.
///
/// After this, every edit has a non-empty claim, an edit that inserts text is
/// an insertion, and no edit is a no-op (ADR-0012 decision 1).
pub(crate) fn normalize(edits: &[TextEdit]) -> Vec<Effective<'_>> {
    let mut effective = Vec::with_capacity(edits.len());

    for (index, edit) in edits.iter().enumerate() {
        let kind = match edit {
            TextEdit::Insert { offset, text } => {
                if text.is_empty() {
                    continue;
                }

                EffectiveKind::Insert {
                    offset: *offset,
                    text,
                }
            }
            TextEdit::Delete { range } => {
                if range.is_empty() {
                    continue;
                }

                EffectiveKind::Delete { range: *range }
            }
            TextEdit::Replace { range, text } => match (range.is_empty(), text.is_empty()) {
                (true, true) => continue,
                (true, false) => EffectiveKind::Insert {
                    offset: range.start,
                    text,
                },
                (false, true) => EffectiveKind::Delete { range: *range },
                (false, false) => EffectiveKind::Replace {
                    range: *range,
                    text,
                },
            },
        };

        effective.push(Effective { index, kind });
    }

    effective
}

/// Every pair of `effective` edits that consume the same text.
///
/// A sweep, not a product: the claims are sorted by `(start, end)` and only the
/// claims that reach past the current start are compared. That is
/// `O(k log k + p)` for `p` reported pairs instead of `O(k^2)` always
/// (ADR-0012 decision 4).
///
/// Two insertions at one offset are not a conflict: the claims intersect, but
/// no text is consumed twice, and the apply order reads them in batch order
/// (ADR-0013 decision 2). Every other intersecting pair is reported on the
/// later edit of the pair, so `first < second`.
pub(crate) fn conflicts(effective: &[&Effective<'_>]) -> Vec<EditProblem> {
    let mut order: Vec<usize> = (0..effective.len()).collect();
    order.sort_by_key(|&position| {
        let claim = effective[position].claim();

        (claim.start.0, claim.end.0, position)
    });

    let mut active: Vec<usize> = Vec::new();
    let mut problems = Vec::new();

    for &position in &order {
        let current = effective[position];
        let claim = current.claim();

        // Claims sorted by start: one still in the window is one that reaches
        // past the current start, i.e. the only kind that can intersect it.
        active.retain(|&other| effective[other].claim().end.0 > claim.start.0);

        for &other in &active {
            let other_edit = effective[other];

            if current.is_insertion() && other_edit.is_insertion() {
                continue;
            }

            let (first, second) = if current.index() < other_edit.index() {
                (current.index(), other_edit.index())
            } else {
                (other_edit.index(), current.index())
            };

            problems.push(EditProblem {
                index: second,
                error: TextError::OverlappingEdits { first, second },
            });
        }

        active.push(position);
    }

    // Within one index the conflicts are ordered by `first`, which is the order
    // a host highlights them in.
    problems.sort_by_key(|problem| match problem.error {
        TextError::OverlappingEdits { first, .. } => (problem.index, first),
        _ => (problem.index, 0),
    });

    problems
}

/// A problem found in a batch edit, together with the offending edit.
///
/// `Document::edit_problems` reports every problem it can find, so a host can
/// highlight all the edits it has to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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

impl std::error::Error for EditProblem {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_span_is_empty_at_its_offset() {
        let edit = TextEdit::insert(TextOffset(4), "x");

        assert_eq!(edit.span(), TextRange::new(TextOffset(4), TextOffset(4)));
    }

    #[test]
    fn delete_span_is_its_range() {
        let range = TextRange::new(TextOffset(1), TextOffset(4));
        let edit = TextEdit::delete(range);

        assert_eq!(edit.span(), range);
    }

    #[test]
    fn replace_span_is_its_range() {
        let range = TextRange::new(TextOffset(1), TextOffset(4));
        let edit = TextEdit::replace(range, "x");

        assert_eq!(edit.span(), range);
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
        let edits = [TextEdit::insert(TextOffset(7), "x")];
        let effective = normalize(&edits);

        assert_eq!(
            effective[0].claim(),
            TextRange::new(TextOffset(7), TextOffset(8))
        );
        assert!(effective[0].is_insertion());
    }

    // -- normalization ----------------------------------------------------

    #[test]
    fn no_op_edits_are_dropped() {
        let edits = [
            TextEdit::insert(TextOffset(0), ""),
            TextEdit::delete(TextRange::new(TextOffset(1), TextOffset(1))),
            TextEdit::replace(TextRange::new(TextOffset(1), TextOffset(1)), ""),
        ];
        let effective = normalize(&edits);

        assert!(effective.is_empty());
    }

    #[test]
    fn an_empty_range_replace_becomes_an_insertion() {
        let edits = [TextEdit::replace(
            TextRange::new(TextOffset(3), TextOffset(3)),
            "x",
        )];
        let effective = normalize(&edits);

        assert_eq!(effective.len(), 1);
        assert!(effective[0].is_insertion());
        assert_eq!(
            effective[0].span(),
            TextRange::new(TextOffset(3), TextOffset(3))
        );
        assert_eq!(
            effective[0].claim(),
            TextRange::new(TextOffset(3), TextOffset(4))
        );
    }

    #[test]
    fn a_replace_without_text_becomes_a_delete() {
        let range = TextRange::new(TextOffset(1), TextOffset(4));
        let edits = [TextEdit::replace(range, "")];
        let effective = normalize(&edits);

        assert_eq!(effective.len(), 1);
        assert!(!effective[0].is_insertion());
        assert_eq!(effective[0].span(), range);
        assert_eq!(effective[0].claim(), range);
        assert_eq!(effective[0].replacement_len(), 0);
    }

    #[test]
    fn indices_still_point_into_the_edit_batch() {
        let edits = [
            TextEdit::insert(TextOffset(0), ""),
            TextEdit::insert(TextOffset(1), "x"),
            TextEdit::delete(TextRange::new(TextOffset(2), TextOffset(2))),
            TextEdit::delete(TextRange::new(TextOffset(3), TextOffset(4))),
        ];
        let effective = normalize(&edits);

        assert_eq!(effective.len(), 2);
        assert_eq!(effective[0].index(), 1);
        assert_eq!(effective[1].index(), 3);
    }

    #[test]
    fn every_surviving_edit_has_a_non_empty_claim() {
        let edits = [
            TextEdit::insert(TextOffset(0), "x"),
            TextEdit::delete(TextRange::new(TextOffset(1), TextOffset(4))),
            TextEdit::replace(TextRange::new(TextOffset(5), TextOffset(9)), "yz"),
        ];
        let effective = normalize(&edits);

        for edit in &effective {
            assert!(
                !edit.claim().is_empty(),
                "empty claim for edit {}",
                edit.index()
            );
        }
    }

    #[test]
    fn a_claim_at_the_last_offset_saturates() {
        let edits = [TextEdit::insert(TextOffset(usize::MAX), "x")];
        let effective = normalize(&edits);

        assert_eq!(
            effective[0].claim(),
            TextRange::new(TextOffset(usize::MAX), TextOffset(usize::MAX))
        );
    }

    // -- conflicts --------------------------------------------------------

    #[test]
    fn overlapping_ranges_conflict() {
        let edits = [
            TextEdit::delete(TextRange::new(TextOffset(0), TextOffset(4))),
            TextEdit::delete(TextRange::new(TextOffset(2), TextOffset(6))),
        ];
        let effective = normalize(&edits);
        let view: Vec<&Effective<'_>> = effective.iter().collect();

        assert_eq!(
            conflicts(&view),
            vec![EditProblem {
                index: 1,
                error: TextError::OverlappingEdits {
                    first: 0,
                    second: 1
                },
            }]
        );
    }

    #[test]
    fn touching_ranges_do_not_conflict() {
        let edits = [
            TextEdit::delete(TextRange::new(TextOffset(0), TextOffset(2))),
            TextEdit::delete(TextRange::new(TextOffset(2), TextOffset(4))),
        ];
        let effective = normalize(&edits);
        let view: Vec<&Effective<'_>> = effective.iter().collect();

        assert!(conflicts(&view).is_empty());
    }

    /// One intent has one spelling: an insertion and an empty-range
    /// replacement are the same edit, so they are treated the same way - both
    /// are insertions at that offset, and two insertions are ordered rather
    /// than rejected (ADR-0012 finding 2, ADR-0013 decision 2).
    #[test]
    fn an_insertion_and_an_empty_range_replace_at_one_offset_are_both_insertions() {
        let edits = [
            TextEdit::insert(TextOffset(1), "X"),
            TextEdit::replace(TextRange::new(TextOffset(1), TextOffset(1)), "Y"),
        ];
        let effective = normalize(&edits);
        let view: Vec<&Effective<'_>> = effective.iter().collect();

        assert_eq!(effective.len(), 2);
        assert!(effective.iter().all(Effective::is_insertion));
        assert!(conflicts(&view).is_empty());
    }

    #[test]
    fn two_insertions_at_one_offset_are_ordered_not_rejected() {
        let edits = [
            TextEdit::insert(TextOffset(1), "a"),
            TextEdit::insert(TextOffset(1), "b"),
        ];
        let effective = normalize(&edits);
        let view: Vec<&Effective<'_>> = effective.iter().collect();

        assert!(conflicts(&view).is_empty());
    }

    #[test]
    fn an_insertion_conflicts_with_a_range_that_covers_it() {
        let edits = [
            TextEdit::replace(TextRange::new(TextOffset(0), TextOffset(6)), "!"),
            TextEdit::insert(TextOffset(3), "x"),
        ];
        let effective = normalize(&edits);
        let view: Vec<&Effective<'_>> = effective.iter().collect();

        assert_eq!(conflicts(&view).len(), 1);
    }

    #[test]
    fn every_pair_is_reported_once() {
        let edits = [
            TextEdit::delete(TextRange::new(TextOffset(0), TextOffset(4))),
            TextEdit::delete(TextRange::new(TextOffset(2), TextOffset(5))),
            TextEdit::delete(TextRange::new(TextOffset(1), TextOffset(3))),
        ];
        let effective = normalize(&edits);
        let view: Vec<&Effective<'_>> = effective.iter().collect();

        assert_eq!(
            conflicts(&view),
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
