use std::fmt;

/// Identifies one state of a `Document`.
///
/// Offsets and ranges are only meaningful together with the revision they were
/// computed against: an edit in front of a position shifts it. A host that
/// caches a position, a search result or a styling run pairs it with the
/// `RevisionId` it was computed from and can then tell whether the cache entry
/// is still valid. See ADR-0005.
///
/// The revision counter starts at [`RevisionId::INITIAL`], which is the state
/// of a freshly created or freshly loaded document. Loading content is not an
/// edit, so it does not advance the counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RevisionId(u64);

impl RevisionId {
    /// The revision of a document that has not been edited yet.
    pub const INITIAL: Self = Self(0);

    /// The revision as an integer, for hosts that store it next to their own data.
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// The revision that follows `self`.
    ///
    /// The counter saturates instead of wrapping. Reaching the limit would
    /// require 2^64 mutations, so saturation is unreachable in practice; it only
    /// guarantees the counter never jumps back to a value an older cache entry
    /// may already use.
    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl From<RevisionId> for u64 {
    fn from(revision: RevisionId) -> u64 {
        revision.0
    }
}

impl fmt::Display for RevisionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_is_zero() {
        assert_eq!(RevisionId::default(), RevisionId::INITIAL);
        assert_eq!(RevisionId::INITIAL.as_u64(), 0);
    }

    #[test]
    fn revisions_increase_by_one() {
        let first = RevisionId::INITIAL.next();
        let second = first.next();

        assert_eq!(first.as_u64(), 1);
        assert_eq!(second.as_u64(), 2);
        assert!(RevisionId::INITIAL < first);
        assert!(first < second);
    }

    #[test]
    fn saturation_never_goes_backwards() {
        let last = RevisionId(u64::MAX);

        assert_eq!(last.next(), last);
        assert!(last.next() >= last);
    }

    #[test]
    fn display_is_short_and_readable() {
        assert_eq!(RevisionId::INITIAL.to_string(), "r0");
        assert_eq!(RevisionId(41).to_string(), "r41");
    }

    #[test]
    fn converts_into_a_plain_integer() {
        let revision = RevisionId(7);

        assert_eq!(u64::from(revision), 7);
    }
}
