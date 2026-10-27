//! A simplified rope.
//!
//! A rope stores the text as a tree of chunks instead of one contiguous
//! buffer. The point of the structure is that an edit only touches the chunks
//! around the edit position, while a read of a range only touches the chunks
//! of that range.
//!
//! This is a prototype: the goal of week 9 is to learn the data structure, not
//! to ship a production storage layer. The limits are listed in ADR-0009.
//!
//! ```text
//!                Internal
//!              /    |     \
//!        Internal  Leaf   Leaf
//!        /   |     "abc"  "def"
//!      Leaf  Leaf
//!     "abc"  "def"
//! ```
//!
//! The shape above is a B+ tree: the leaves hold the text, the internal nodes
//! hold children, and all leaves sit at the same depth. That is what keeps the
//! tree logarithmic in the number of chunks, instead of degenerating into a
//! list.

use std::borrow::Cow;

use super::TextStore;
use crate::error::TextResult;
use crate::{TextOffset, TextRange};

/// Maximum number of bytes in one leaf.
const MAX_CHUNK: usize = 1024;

/// Maximum number of children in one internal node.
const MAX_BRANCHING: usize = 8;

/// One node of the rope.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    /// A chunk of text.
    Leaf { text: String },

    /// A group of children, with the length of the text below them.
    Internal { children: Vec<Node>, len: usize },
}

impl Node {
    /// Length of the text below this node.
    fn len(&self) -> usize {
        match self {
            Node::Leaf { text } => text.len(),
            Node::Internal { len, .. } => *len,
        }
    }

    /// Number of leaves below this node.
    fn leaf_count(&self) -> usize {
        match self {
            Node::Leaf { .. } => 1,
            Node::Internal { children, .. } => children.iter().map(Node::leaf_count).sum(),
        }
    }

    /// Depth of the tree below this node; a leaf has depth 1.
    fn depth(&self) -> usize {
        match self {
            Node::Leaf { .. } => 1,
            Node::Internal { children, .. } => {
                1 + children.iter().map(Node::depth).max().unwrap_or(0)
            }
        }
    }

    /// Builds an internal node from its children.
    fn internal(children: Vec<Node>) -> Node {
        let len = children.iter().map(Node::len).sum();

        Node::Internal { children, len }
    }

    /// Builds a tree from leaves, all at the same depth.
    fn from_leaves(leaves: Vec<Node>) -> Option<Node> {
        match leaves.len() {
            0 => None,
            1 => leaves.into_iter().next(),
            _ => {
                let middle = leaves.len() / 2;
                let mut iter = leaves.into_iter();
                let left: Vec<Node> = iter.by_ref().take(middle).collect();
                let right: Vec<Node> = iter.collect();

                let left = Node::from_leaves(left).expect("the left half is not empty");
                let right = Node::from_leaves(right).expect("the right half is not empty");

                Some(Node::internal(vec![left, right]))
            }
        }
    }

    /// The child that contains `offset`, and the offset inside that child.
    ///
    /// An offset at the very end of the text belongs to the last child, so
    /// that appending extends the last chunk instead of creating an empty one.
    fn locate(children: &[Node], offset: usize) -> (usize, usize) {
        let mut start = 0;

        for (index, child) in children.iter().enumerate() {
            let child_len = child.len();

            if offset < start + child_len {
                return (index, offset - start);
            }

            start += child_len;
        }

        let last = children.len() - 1;

        (last, children[last].len())
    }
}

/// The result of inserting into a node.
enum Inserted {
    /// The node still fits.
    Fits(Node),

    /// The node had to be split into several nodes of the same level.
    Split(Vec<Node>),
}

/// Splits text into chunks of at most `MAX_CHUNK` bytes, on character
/// boundaries.
fn chunk_text(text: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut chunk_start = 0;
    let mut last_boundary = 0;

    for (index, _) in text.char_indices() {
        if index - chunk_start >= MAX_CHUNK {
            chunks.push(text[chunk_start..last_boundary].to_string());
            chunk_start = last_boundary;
        }

        last_boundary = index;
    }

    if chunk_start < text.len() {
        chunks.push(text[chunk_start..].to_string());
    }

    chunks
}
impl Node {
    /// Inserts `text` at `offset`, which has to be a character boundary.
    fn insert(self, offset: usize, text: &str) -> Inserted {
        match self {
            Node::Leaf { text: leaf_text } => {
                let mut merged = String::with_capacity(leaf_text.len() + text.len());
                merged.push_str(&leaf_text[..offset]);
                merged.push_str(text);
                merged.push_str(&leaf_text[offset..]);

                if merged.len() <= MAX_CHUNK {
                    return Inserted::Fits(Node::Leaf { text: merged });
                }

                // The chunk is full: cut it into pieces that fit again.
                let pieces: Vec<Node> = chunk_text(&merged)
                    .into_iter()
                    .map(|text| Node::Leaf { text })
                    .collect();

                Inserted::Split(pieces)
            }
            Node::Internal { mut children, .. } => {
                let (index, local) = Node::locate(&children, offset);
                let child = children.remove(index);

                match child.insert(local, text) {
                    Inserted::Fits(new_child) => {
                        children.insert(index, new_child);

                        Inserted::Fits(Node::internal(children))
                    }
                    Inserted::Split(pieces) => {
                        for (offset, piece) in pieces.into_iter().enumerate() {
                            children.insert(index + offset, piece);
                        }

                        if children.len() > MAX_BRANCHING {
                            // The node is full: split it in the middle.
                            //
                            // The children are moved, not copied: a clone here
                            // would copy the whole subtree on every insert.
                            // An even split also keeps every group of at least
                            // two children, so a full node never creates a
                            // one-child level.
                            let middle = children.len() / 2;
                            let right = children.split_off(middle);

                            Inserted::Split(vec![Node::internal(children), Node::internal(right)])
                        } else {
                            Inserted::Fits(Node::internal(children))
                        }
                    }
                }
            }
        }
    }

    /// Removes the text in `range`, in local coordinates.
    ///
    /// Returns the remaining node and the number of removed bytes.
    fn delete(self, range: TextRange) -> (Option<Node>, usize) {
        match self {
            Node::Leaf { text } => {
                let from = range.start.0.min(text.len());
                let to = range.end.0.min(text.len());

                let mut remaining = String::with_capacity(text.len() - (to - from));
                remaining.push_str(&text[..from]);
                remaining.push_str(&text[to..]);

                let removed = to - from;

                if remaining.is_empty() {
                    (None, removed)
                } else {
                    (Some(Node::Leaf { text: remaining }), removed)
                }
            }
            Node::Internal { children, .. } => {
                let mut kept: Vec<Node> = Vec::with_capacity(children.len());
                let mut start = 0;
                let mut removed = 0;

                for child in children {
                    let child_len = child.len();
                    let child_start = start;
                    start += child_len;

                    let outside =
                        child_start >= range.end.0 || child_start + child_len <= range.start.0;

                    if outside {
                        kept.push(child);
                        continue;
                    }

                    let from = range.start.0.saturating_sub(child_start).min(child_len);
                    let to = range.end.0.saturating_sub(child_start).min(child_len);
                    let local = TextRange::new(TextOffset(from), TextOffset(to));

                    let (new_child, removed_bytes) = child.delete(local);
                    removed += removed_bytes;

                    if let Some(new_child) = new_child {
                        kept.push(new_child);
                    }
                }

                if kept.is_empty() {
                    (None, removed)
                } else {
                    (Some(Node::internal(kept)), removed)
                }
            }
        }
    }

    /// The text below this node.
    fn collect_text(&self, out: &mut String) {
        match self {
            Node::Leaf { text } => out.push_str(text),
            Node::Internal { children, .. } => {
                for child in children {
                    child.collect_text(out);
                }
            }
        }
    }

    /// The text of `range`, in local coordinates.
    fn collect_range(&self, range: TextRange, start: usize, out: &mut String) {
        let node_start = start;
        let node_end = start + self.len();

        if node_end <= range.start.0 || node_start >= range.end.0 {
            return;
        }

        match self {
            Node::Leaf { text } => {
                let from = range.start.0.max(node_start) - node_start;
                let to = range.end.0.min(node_end) - node_start;

                out.push_str(&text[from..to]);
            }
            Node::Internal { children, .. } => {
                let mut child_start = node_start;

                for child in children {
                    child.collect_range(range, child_start, out);
                    child_start += child.len();
                }
            }
        }
    }

    /// Whether `offset` is on a character boundary.
    fn boundary_at(&self, offset: usize, start: usize) -> bool {
        match self {
            Node::Leaf { text } => text.is_char_boundary(offset - start),
            Node::Internal { children, .. } => {
                let (index, _) = Node::locate(children, offset);
                let child_start = start + children[..index].iter().map(Node::len).sum::<usize>();

                children[index].boundary_at(offset, child_start)
            }
        }
    }

    /// The chunks of this node, in order.
    fn collect_leaves(&self, out: &mut Vec<String>) {
        match self {
            Node::Leaf { text } => out.push(text.clone()),
            Node::Internal { children, .. } => {
                for child in children {
                    child.collect_leaves(out);
                }
            }
        }
    }

    /// The length of the longest leaf below this node.
    fn longest_leaf(&self) -> usize {
        match self {
            Node::Leaf { text } => text.len(),
            Node::Internal { children, .. } => {
                children.iter().map(Node::longest_leaf).max().unwrap_or(0)
            }
        }
    }
}
/// A rope: the text as a tree of chunks.
///
/// The rope is the storage structure the week 8 evaluation selected for the
/// second backend. This implementation is a prototype; its limits are
/// documented in ADR-0009.
///
/// ```text
/// rope.insert(TextOffset(0), "Hello");
/// rope.text();            // "Hello"
/// rope.text_in(range);    // a range of the text, without copying the rest
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rope {
    root: Option<Node>,
    len: usize,
}

impl Rope {
    /// An empty rope.
    pub fn new() -> Self {
        Self::default()
    }

    /// A rope holding `text`.
    pub fn from_text(text: &str) -> Self {
        let leaves: Vec<Node> = chunk_text(text)
            .into_iter()
            .map(|text| Node::Leaf { text })
            .collect();

        Self {
            root: Node::from_leaves(leaves),
            len: text.len(),
        }
    }

    /// Length of the text in UTF-8 bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The whole text.
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(self.len);

        if let Some(root) = &self.root {
            root.collect_text(&mut out);
        }

        out
    }

    /// A range of the text.
    ///
    /// Only the chunks that the range touches are copied.
    pub fn text_in(&self, range: TextRange) -> String {
        let mut out = String::new();

        if let Some(root) = &self.root {
            root.collect_range(range, 0, &mut out);
        }

        out
    }

    /// Whether `offset` is on a UTF-8 character boundary.
    ///
    /// An offset past the end of the text is not a boundary.
    pub fn is_char_boundary(&self, offset: TextOffset) -> bool {
        if offset.0 > self.len {
            return false;
        }

        match &self.root {
            Some(root) => root.boundary_at(offset.0, 0),
            None => offset.0 == 0,
        }
    }

    /// Inserts `text` at `offset`.
    ///
    /// The offset has to be a character boundary. The store validates it
    /// before it gets here.
    pub fn insert(&mut self, offset: TextOffset, text: &str) {
        if text.is_empty() {
            return;
        }

        let mut at = offset.0;

        // A large text is inserted chunk by chunk, so that no leaf ever grows
        // beyond the chunk limit.
        for chunk in chunk_text(text) {
            self.insert_chunk(at, &chunk);
            at += chunk.len();
        }
    }

    /// Deletes `range`.
    ///
    /// The range has to be valid. The store validates it before it gets here.
    pub fn delete(&mut self, range: TextRange) {
        let root = match self.root.take() {
            Some(root) => root,
            None => return,
        };

        let (new_root, removed) = root.delete(range);

        self.root = new_root;
        self.len -= removed;
    }
    /// Splits the rope into two ropes at `offset`.
    ///
    /// The left rope holds the text before `offset`, the right rope the text
    /// from `offset` on.
    ///
    /// This is the teaching version: it rebuilds both sides from the chunks,
    /// which costs O(n). A production rope splits the path to the offset
    /// instead and keeps the tree (ADR-0009).
    pub fn split_at(&self, offset: TextOffset) -> (Rope, Rope) {
        let mut leaves: Vec<String> = Vec::new();

        if let Some(root) = &self.root {
            root.collect_leaves(&mut leaves);
        }

        let mut left: Vec<Node> = Vec::new();
        let mut right: Vec<Node> = Vec::new();
        let mut start = 0;
        let split = offset.0.min(self.len);

        for leaf in leaves {
            let leaf_len = leaf.len();

            if start + leaf_len <= split {
                left.push(Node::Leaf { text: leaf });
            } else if start >= split {
                right.push(Node::Leaf { text: leaf });
            } else {
                let local = split - start;
                let (a, b) = leaf.split_at(local);

                left.push(Node::Leaf {
                    text: a.to_string(),
                });
                right.push(Node::Leaf {
                    text: b.to_string(),
                });
            }

            start += leaf_len;
        }

        (
            Rope {
                root: Node::from_leaves(left),
                len: split,
            },
            Rope {
                root: Node::from_leaves(right),
                len: self.len - split,
            },
        )
    }

    /// The text of `other` appended to this rope.
    ///
    /// Like [`Rope::split_at`], this is the teaching version: it appends chunk
    /// by chunk instead of merging the two trees.
    pub fn concat(&self, other: &Rope) -> Rope {
        let mut result = self.clone();
        let mut leaves: Vec<String> = Vec::new();

        if let Some(root) = &other.root {
            root.collect_leaves(&mut leaves);
        }

        for leaf in leaves {
            let at = result.len();
            result.insert_chunk(at, &leaf);
        }

        result
    }

    /// Number of chunks the text is stored in.
    pub fn leaf_count(&self) -> usize {
        match &self.root {
            Some(root) => root.leaf_count(),
            None => 0,
        }
    }

    /// Depth of the tree.
    pub fn depth(&self) -> usize {
        match &self.root {
            Some(root) => root.depth(),
            None => 0,
        }
    }

    /// Length of the longest chunk.
    pub fn longest_leaf(&self) -> usize {
        match &self.root {
            Some(root) => root.longest_leaf(),
            None => 0,
        }
    }

    fn insert_chunk(&mut self, offset: usize, text: &str) {
        let root = match self.root.take() {
            Some(root) => root,
            None => {
                self.root = Some(Node::Leaf {
                    text: text.to_string(),
                });
                self.len += text.len();

                return;
            }
        };

        self.root = Some(match root.insert(offset, text) {
            Inserted::Fits(node) => node,
            Inserted::Split(nodes) => Node::internal(nodes),
        });
        self.len += text.len();
    }
}
/// A [`TextStore`] backed by a rope.
///
/// This is the second storage backend of the core. It exists to prove that the
/// storage boundary of ADR-0008 really works for a backend that does not keep
/// the text in one buffer, and to give week 10 a second implementation to
/// measure against the string backend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RopeTextStore {
    rope: Rope,
}

impl RopeTextStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// A store holding `text`.
    pub fn from_text(text: &str) -> Self {
        Self {
            rope: Rope::from_text(text),
        }
    }

    /// The rope behind this store.
    pub fn rope(&self) -> &Rope {
        &self.rope
    }
}

impl From<Rope> for RopeTextStore {
    fn from(rope: Rope) -> Self {
        Self { rope }
    }
}

impl TextStore for RopeTextStore {
    fn len(&self) -> usize {
        self.rope.len()
    }

    fn text(&self) -> Cow<'_, str> {
        // A rope cannot hand out a borrowed whole text: the text lives in
        // many chunks. The caller gets a snapshot instead.
        Cow::Owned(self.rope.text())
    }

    fn text_in(&self, range: TextRange) -> TextResult<Cow<'_, str>> {
        self.validate_range(range)?;

        Ok(Cow::Owned(self.rope.text_in(range)))
    }

    fn insert(&mut self, offset: TextOffset, text: &str) -> TextResult<()> {
        self.validate_offset(offset)?;
        self.rope.insert(offset, text);
        Ok(())
    }

    fn delete(&mut self, range: TextRange) -> TextResult<()> {
        self.validate_range(range)?;
        self.rope.delete(range);
        Ok(())
    }

    fn is_char_boundary(&self, offset: TextOffset) -> bool {
        self.rope.is_char_boundary(offset)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TextError;
    use crate::text::StringTextStore;

    /// A deterministic pseudo random number generator, so that a failure can
    /// be reproduced without a dependency.
    fn next_random(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);

        *state >> 33
    }

    /// A random offset that is on a character boundary of `text`.
    fn random_boundary(text: &str, state: &mut u64) -> usize {
        let mut offset = (next_random(state) as usize) % (text.len() + 1);

        while offset > 0 && !text.is_char_boundary(offset) {
            offset -= 1;
        }

        offset
    }

    /// A short random text, sometimes with multi-byte characters.
    fn random_text(state: &mut u64) -> String {
        let samples = ["a", "bc", "中", "文", "x", "hello", "你好"];

        samples[(next_random(state) as usize) % samples.len()].to_string()
    }

    #[test]
    fn an_empty_rope_has_no_text() {
        let rope = Rope::new();

        assert!(rope.is_empty());
        assert_eq!(rope.len(), 0);
        assert_eq!(rope.text(), "");
        assert_eq!(rope.leaf_count(), 0);
        assert_eq!(rope.depth(), 0);
    }

    #[test]
    fn a_rope_keeps_the_text_it_was_built_from() {
        let rope = Rope::from_text("Hello");

        assert_eq!(rope.text(), "Hello");
        assert_eq!(rope.len(), 5);
        assert_eq!(rope.leaf_count(), 1);
    }

    #[test]
    fn text_is_inserted_at_the_right_place() {
        let mut rope = Rope::from_text("Hllo");

        rope.insert(TextOffset(1), "e");

        assert_eq!(rope.text(), "Hello");
    }

    #[test]
    fn text_is_appended_at_the_end() {
        let mut rope = Rope::from_text("Hello");

        rope.insert(TextOffset(5), " world");

        assert_eq!(rope.text(), "Hello world");
    }

    #[test]
    fn a_range_is_deleted() {
        let mut rope = Rope::from_text("Hello world");

        rope.delete(TextRange::new(TextOffset(5), TextOffset(11)));

        assert_eq!(rope.text(), "Hello");
        assert_eq!(rope.len(), 5);
    }

    #[test]
    fn deleting_everything_leaves_an_empty_rope() {
        let mut rope = Rope::from_text("Hello");

        rope.delete(TextRange::new(TextOffset(0), TextOffset(5)));

        assert!(rope.is_empty());
        assert_eq!(rope.text(), "");
        assert_eq!(rope.leaf_count(), 0);
    }

    #[test]
    fn a_range_of_the_text_is_read_without_the_rest() {
        let rope = Rope::from_text("Hello world");

        assert_eq!(
            rope.text_in(TextRange::new(TextOffset(0), TextOffset(5))),
            "Hello"
        );
        assert_eq!(
            rope.text_in(TextRange::new(TextOffset(6), TextOffset(11))),
            "world"
        );
        assert_eq!(
            rope.text_in(TextRange::new(TextOffset(3), TextOffset(3))),
            ""
        );
    }

    #[test]
    fn character_boundaries_follow_the_utf8_layout() {
        let rope = Rope::from_text("你好");

        assert!(rope.is_char_boundary(TextOffset(0)));
        assert!(rope.is_char_boundary(TextOffset(3)));
        assert!(rope.is_char_boundary(TextOffset(6)));
        assert!(!rope.is_char_boundary(TextOffset(1)));
        assert!(!rope.is_char_boundary(TextOffset(2)));
        assert!(!rope.is_char_boundary(TextOffset(7)));
    }
    #[test]
    fn a_large_text_is_stored_in_chunks() {
        let mut rope = Rope::new();
        let text = "a".repeat(10_000);

        rope.insert(TextOffset(0), &text);

        assert_eq!(rope.len(), 10_000);
        assert_eq!(rope.text().len(), 10_000);
        assert!(rope.leaf_count() > 1);
        assert!(rope.longest_leaf() <= MAX_CHUNK);
    }

    #[test]
    fn a_chunk_never_grows_beyond_the_limit() {
        let mut rope = Rope::from_text(&"a".repeat(MAX_CHUNK - 10));

        for _ in 0..50 {
            rope.insert(TextOffset(0), &"b".repeat(100));
        }

        assert!(rope.longest_leaf() <= MAX_CHUNK);
    }

    #[test]
    fn the_tree_stays_shallow_while_it_grows() {
        let mut rope = Rope::new();

        for index in 0..2_000 {
            rope.insert(TextOffset(0), &index.to_string());
        }

        // A tree over 2000 chunks with a branching factor of 8 needs about
        // four levels; it must not degenerate into a list.
        assert!(rope.depth() <= 8, "depth was {}", rope.depth());
    }

    #[test]
    fn splitting_cuts_the_text_in_two() {
        let rope = Rope::from_text("Hello world");

        let (left, right) = rope.split_at(TextOffset(5));

        assert_eq!(left.text(), "Hello");
        assert_eq!(right.text(), " world");
        assert_eq!(left.len() + right.len(), rope.len());
    }

    #[test]
    fn splitting_at_the_start_or_the_end() {
        let rope = Rope::from_text("Hello");

        let (left, right) = rope.split_at(TextOffset(0));
        assert_eq!(left.text(), "");
        assert_eq!(right.text(), "Hello");

        let (left, right) = rope.split_at(TextOffset(5));
        assert_eq!(left.text(), "Hello");
        assert_eq!(right.text(), "");
    }

    #[test]
    fn concatenating_appends_the_text() {
        let left = Rope::from_text("Hello ");
        let right = Rope::from_text("world");

        let joined = left.concat(&right);

        assert_eq!(joined.text(), "Hello world");
        assert_eq!(joined.len(), 11);
    }

    /// The rope and the string backend have to behave the same. This is the
    /// test that matters: the rope is only useful if it is a drop-in
    /// replacement.
    #[test]
    fn the_rope_behaves_like_the_string_backend() {
        let mut state = 0x5eed_1234_u64;
        let mut rope = Rope::new();
        let mut mirror = String::new();

        for _ in 0..500 {
            if mirror.is_empty() || next_random(&mut state).is_multiple_of(3) {
                let offset = random_boundary(&mirror, &mut state);
                let text = random_text(&mut state);

                rope.insert(TextOffset(offset), &text);
                mirror.insert_str(offset, &text);
            } else {
                let first = random_boundary(&mirror, &mut state);
                let second = random_boundary(&mirror, &mut state);
                let (start, end) = if first <= second {
                    (first, second)
                } else {
                    (second, first)
                };

                rope.delete(TextRange::new(TextOffset(start), TextOffset(end)));
                mirror.replace_range(start..end, "");
            }

            assert_eq!(rope.text(), mirror);
            assert_eq!(rope.len(), mirror.len());
        }
    }

    #[test]
    fn the_store_reads_and_writes_the_text() {
        let mut store = RopeTextStore::from_text("Hello");

        store.insert(TextOffset(5), " world").unwrap();

        assert_eq!(store.text(), "Hello world");
        assert_eq!(store.len(), 11);
        assert_eq!(
            store
                .text_in(TextRange::new(TextOffset(0), TextOffset(5)))
                .unwrap(),
            "Hello"
        );
    }

    #[test]
    fn the_store_reports_invalid_positions() {
        let mut store = RopeTextStore::from_text("你好");

        assert_eq!(
            store.insert(TextOffset(1), "x"),
            Err(TextError::NotCharBoundary { offset: 1 })
        );
        assert_eq!(
            store.insert(TextOffset(99), "x"),
            Err(TextError::OffsetOutOfRange { offset: 99, len: 6 })
        );
        assert_eq!(store.text(), "你好");
    }

    #[test]
    fn both_backends_answer_the_same_questions() {
        let mut string_store = StringTextStore::new();
        let mut rope_store = RopeTextStore::new();

        for store in [
            &mut string_store as &mut dyn TextStore,
            &mut rope_store as &mut dyn TextStore,
        ] {
            store.insert(TextOffset(0), "Hello").unwrap();
            store.insert(TextOffset(5), " 世界").unwrap();
        }

        assert_eq!(string_store.text(), rope_store.text());
        assert_eq!(string_store.len(), rope_store.len());
        assert_eq!(
            string_store
                .text_in(TextRange::new(TextOffset(0), TextOffset(5)))
                .unwrap(),
            rope_store
                .text_in(TextRange::new(TextOffset(0), TextOffset(5)))
                .unwrap()
        );
    }
}
