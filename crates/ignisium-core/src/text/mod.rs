mod rope;
mod store;

pub use rope::{Rope, RopeTextStore};
pub use store::{StringTextStore, TextStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextOffset(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: TextOffset,
    pub end: TextOffset,
}

impl TextRange {
    /// A half-open range `[start, end)`.
    ///
    /// The endpoints are not validated here: a range is checked against the
    /// text it is used on, which reports `TextError::InvalidRange` or
    /// `TextError::OffsetOutOfRange` (ADR-0003).
    pub fn new(start: TextOffset, end: TextOffset) -> Self {
        Self { start, end }
    }

    /// Length of the range in UTF-8 bytes, `0` for an inverted range.
    ///
    /// An inverted range is invalid input rather than a programming error the
    /// library should panic on, so the subtraction saturates instead.
    pub fn len(&self) -> usize {
        self.end.0.saturating_sub(self.start.0)
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Whether two half-open ranges share at least one position.
    ///
    /// Touching ranges do not intersect: `[0, 2)` and `[2, 4)` can both be
    /// deleted safely. Batch edits use this to reject two edits that claim the
    /// same text (ADR-0006).
    pub fn intersects(&self, other: &Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_length_is_the_distance_between_the_endpoints() {
        let range = TextRange::new(TextOffset(1), TextOffset(4));

        assert_eq!(range.len(), 3);
        assert!(!range.is_empty());
    }

    #[test]
    fn empty_range_has_no_length() {
        let range = TextRange::new(TextOffset(2), TextOffset(2));

        assert_eq!(range.len(), 0);
        assert!(range.is_empty());
    }

    #[test]
    fn inverted_range_reports_zero_instead_of_panicking() {
        let range = TextRange::new(TextOffset(4), TextOffset(1));

        assert_eq!(range.len(), 0);
    }

    #[test]
    fn ranges_intersect_when_they_share_a_position() {
        let range = TextRange::new(TextOffset(0), TextOffset(4));

        assert!(range.intersects(&TextRange::new(TextOffset(3), TextOffset(6))));
        assert!(range.intersects(&TextRange::new(TextOffset(0), TextOffset(1))));
        assert!(range.intersects(&range));
    }

    #[test]
    fn touching_ranges_do_not_intersect() {
        let range = TextRange::new(TextOffset(0), TextOffset(4));

        assert!(!range.intersects(&TextRange::new(TextOffset(4), TextOffset(8))));
        assert!(!range.intersects(&TextRange::new(TextOffset(4), TextOffset(4))));
        assert!(!range.intersects(&TextRange::new(TextOffset(0), TextOffset(0))));
    }
}
