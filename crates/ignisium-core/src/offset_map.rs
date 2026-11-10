//! Where a batch moved the text.
//!
//! An edit in front of a position shifts it, so a host that holds an offset
//! across a batch (a cursor, a selection, a search hit, an undo record) has to
//! ask where that offset is now. [`OffsetMap`] answers that question for the
//! batch that just finished, in `O(log n)`, without re-reading the text
//! (ADR-0011 decision 4, ADR-0013 decision 1).

use crate::{TextOffset, TextRange};

/// One applied edit, in the coordinates of the text *before* the batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mapping {
    /// The part of the old text the edit acted on; empty for an insertion.
    old: TextRange,

    /// Total length change of every edit in front of this one.
    shift_before: isize,

    /// Total length change of every edit up to and including this one.
    shift_after: isize,
}

/// Where the offsets of the text before a batch point in the text after it.
///
/// The rule, in one place, so that no host has to invent it (ADR-0013
/// decision 1):
///
/// - a position **before** an edit keeps its distance (`map(p) == p`);
/// - a position **inside** a replaced or deleted range maps to the **start of
///   the replacement**: text that was deleted has no position of its own, and
///   a cursor in the middle of it belongs to what replaced it;
/// - a position **at or after** an edit shifts by the length difference.
///
/// An insertion is the common case of the third rule: its replaced range is
/// empty, so there is no "inside", and a position at the insertion point ends
/// up behind the inserted text.
///
/// The map is built from the edits a batch applied, so it stays valid only as
/// long as the text it was reported with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OffsetMap {
    /// Sorted by `old.start`, with the cumulative shift of each entry.
    mappings: Vec<Mapping>,
}

impl OffsetMap {
    /// An empty map: nothing moved.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the map of a batch from its edits.
    ///
    /// Each item is the range an edit acted on and the length of the text that
    /// replaced it. The edits may arrive in any order; they may touch but not
    /// overlap.
    pub(crate) fn from_edits(edits: impl IntoIterator<Item = (TextRange, usize)>) -> Self {
        let mut parts: Vec<(TextRange, usize)> = edits.into_iter().collect();
        parts.sort_by_key(|(old, _)| (old.start.0, old.end.0));

        let mut mappings = Vec::with_capacity(parts.len());
        let mut shift = 0isize;

        for (old, new_len) in parts {
            let delta = (new_len as isize).saturating_sub(old.len() as isize);
            let shift_before = shift;

            shift = shift.saturating_add(delta);

            mappings.push(Mapping {
                old,
                shift_before,
                shift_after: shift,
            });
        }

        Self { mappings }
    }

    /// Whether nothing moved.
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }

    /// Number of edits the map was built from.
    pub fn len(&self) -> usize {
        self.mappings.len()
    }

    /// Where `offset`, written against the text before the batch, points now.
    ///
    /// The answer is a total function: an offset past the end of the old text
    /// is shifted like every other position behind the last edit.
    pub fn map(&self, offset: TextOffset) -> TextOffset {
        let index = self
            .mappings
            .partition_point(|mapping| mapping.old.start.0 <= offset.0);

        if index == 0 {
            return offset;
        }

        let mapping = &self.mappings[index - 1];

        if offset.0 < mapping.old.end.0 {
            // The start of the replacement, in post-batch coordinates: the
            // edits in front of this one moved it too.
            return TextOffset(
                mapping
                    .old
                    .start
                    .0
                    .saturating_add_signed(mapping.shift_before),
            );
        }

        TextOffset(offset.0.saturating_add_signed(mapping.shift_after))
    }

    /// Where `range` points now.
    ///
    /// Both endpoints follow [`OffsetMap::map`], so a range that was entirely
    /// replaced collapses to an empty range at the start of the replacement.
    pub fn map_range(&self, range: TextRange) -> TextRange {
        TextRange::new(self.map(range.start), self.map(range.end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: usize, end: usize) -> TextRange {
        TextRange::new(TextOffset(start), TextOffset(end))
    }

    /// `"abc" -> "aXYbc"` by inserting `"XY"` at 1.
    fn insertion() -> OffsetMap {
        OffsetMap::from_edits([(range(1, 1), 2)])
    }

    /// `"abcdef" -> "aef"` by deleting `[1, 4)`.
    fn deletion() -> OffsetMap {
        OffsetMap::from_edits([(range(1, 4), 0)])
    }

    /// `"abcdef" -> "aXYef"` by replacing `[1, 4)`.
    fn replacement() -> OffsetMap {
        OffsetMap::from_edits([(range(1, 4), 2)])
    }

    #[test]
    fn an_empty_map_moves_nothing() {
        let map = OffsetMap::new();

        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.map(TextOffset(7)), TextOffset(7));
    }

    #[test]
    fn a_position_before_an_edit_keeps_its_distance() {
        assert_eq!(insertion().map(TextOffset(0)), TextOffset(0));
        assert_eq!(deletion().map(TextOffset(0)), TextOffset(0));
        assert_eq!(replacement().map(TextOffset(0)), TextOffset(0));
    }

    #[test]
    fn a_position_at_an_insertion_moves_behind_the_inserted_text() {
        assert_eq!(insertion().map(TextOffset(1)), TextOffset(3));
        assert_eq!(insertion().map(TextOffset(2)), TextOffset(4));
    }

    #[test]
    fn a_position_inside_a_deleted_range_maps_to_its_start() {
        assert_eq!(deletion().map(TextOffset(1)), TextOffset(1));
        assert_eq!(deletion().map(TextOffset(3)), TextOffset(1));
    }

    #[test]
    fn a_position_inside_a_replaced_range_maps_to_the_start_of_the_replacement() {
        assert_eq!(replacement().map(TextOffset(1)), TextOffset(1));
        assert_eq!(replacement().map(TextOffset(2)), TextOffset(1));
        assert_eq!(replacement().map(TextOffset(3)), TextOffset(1));
    }

    #[test]
    fn a_position_after_an_edit_shifts_by_the_difference() {
        assert_eq!(deletion().map(TextOffset(4)), TextOffset(1));
        assert_eq!(deletion().map(TextOffset(6)), TextOffset(3));
        assert_eq!(replacement().map(TextOffset(4)), TextOffset(3));
        assert_eq!(replacement().map(TextOffset(6)), TextOffset(5));
    }

    #[test]
    fn shifts_of_several_edits_add_up() {
        // "one two three" -> "1 two 3"
        let map = OffsetMap::from_edits([(range(0, 3), 1), (range(8, 13), 1)]);

        assert_eq!(map.map(TextOffset(0)), TextOffset(0));
        assert_eq!(map.map(TextOffset(3)), TextOffset(1));
        assert_eq!(map.map(TextOffset(4)), TextOffset(2));
        assert_eq!(map.map(TextOffset(8)), TextOffset(6));
        assert_eq!(map.map(TextOffset(13)), TextOffset(7));
    }

    #[test]
    fn the_order_the_edits_arrive_in_does_not_matter() {
        let forward = OffsetMap::from_edits([(range(0, 3), 1), (range(8, 13), 1)]);
        let backward = OffsetMap::from_edits([(range(8, 13), 1), (range(0, 3), 1)]);

        assert_eq!(forward, backward);
    }

    #[test]
    fn a_range_follows_both_of_its_endpoints() {
        let map = OffsetMap::from_edits([(range(2, 4), 1)]);

        assert_eq!(map.map_range(range(0, 2)), range(0, 2));
        // A start inside the replaced range collapses to the replacement, an
        // end behind it shifts: what is left of the range is the replacement.
        assert_eq!(map.map_range(range(2, 4)), range(2, 3));
        assert_eq!(map.map_range(range(4, 8)), range(3, 7));
    }

    #[test]
    fn the_start_of_a_later_replacement_is_already_shifted() {
        // "one two three" -> "1 two 3": the second replacement now starts at 6,
        // so a position inside it has to map there, not to its old offset 8.
        let map = OffsetMap::from_edits([(range(0, 3), 1), (range(8, 13), 1)]);

        assert_eq!(map.map(TextOffset(9)), TextOffset(6));
        assert_eq!(map.map(TextOffset(12)), TextOffset(6));
    }

    #[test]
    fn two_insertions_at_one_offset_are_both_counted() {
        let map = OffsetMap::from_edits([(range(1, 1), 1), (range(1, 1), 2)]);

        assert_eq!(map.map(TextOffset(0)), TextOffset(0));
        assert_eq!(map.map(TextOffset(1)), TextOffset(4));
    }
}
