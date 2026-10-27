# ADR-0009: Rope Prototype

## Status

Accepted.

Implements the rope prototype of week 9, the second implementation of
`TextStore`. Follows ADR-0007 (the rope is the next backend) and ADR-0008 (the
trait it is implemented against).

## Context

Week 8 selected the rope as the next storage backend and week 9 reshaped
`TextStore` so that a chunked backend can exist at all. Week 9 builds the
prototype itself.

The plan is explicit about the goal:

> 重点：学数据结构，而不是追求立即可用于生产。

So the prototype has to be **correct** and **honest about its limits**. It is
not a production storage layer, and this record says exactly where it stops.

## Decision

### 1. The rope is a B+ tree of chunks

```text
                Internal
              /    |     \
        Internal  Leaf   Leaf
        /   |     "abc"  "def"
      Leaf  Leaf
     "abc"  "def"
```

- **Leaves** hold the text, at most `MAX_CHUNK` (1 KiB) bytes each.
- **Internal nodes** hold up to `MAX_BRANCHING` (8) children and the length of
  the text below them.
- **All leaves sit at the same depth.** That is what keeps the tree
  logarithmic in the number of chunks instead of degenerating into a list.

The chunk limit is the rule that answers the long line requirement of the week
8 evaluation (R4): a line is not a chunk. A 100 MB single line is a hundred
thousand chunks, not one enormous leaf, so no operation ever has to build an
object proportional to the whole line.

### 2. The operations

| Operation | How it works | Cost |
|---|---|---|
| `insert` | descend to the leaf, insert there, split the leaf when it is full, split the parents on the way up | O(log n) plus the copied chunk |
| `delete` | walk the affected leaves, drop the covered parts, drop the parents that become empty | O(log n + removed chunks) |
| `text_in` | descend to the first leaf, copy the chunks the range touches | O(log n + range) |
| `text` | copy every chunk | O(n) |
| `is_char_boundary` | descend to the leaf and ask the chunk | O(log n) |

`split_at` and `concat` exist because the plan asks for them, but they are the
**teaching versions**: `split_at` rebuilds both sides from the chunk list
(O(n)) and `concat` appends chunk by chunk (O(m log n)). A production rope
splits the path to the offset and merges the two trees in place. The
production versions are not needed to learn the structure, and pretending
otherwise would hide the interesting part.

### 3. The prototype is a backend, not a replacement

`RopeTextStore` implements `TextStore` and works with `Document` through
`Document::from_store`:

```rust
let document = Document::from_store(RopeTextStore::from_text("Hello"));
```

`StringTextStore` stays the default. Which backend is the default for large
documents is decided in week 10 with measurements, not in week 9 with
intentions (ADR-0007).

### 4. Two bugs the prototype found, and what they teach

**Cloning a subtree instead of moving it.** The first version of the split
built the new level with `group.to_vec()`, which *clones* every node below it.
The symptom was a test suite that took 29 minutes instead of 0.03 seconds,
because every insert copied the whole tree. The rule for a tree structure:
splitting moves nodes, it never copies them. A clone of a subtree is a bug
that only shows up as a mysterious slowdown.

**Splitting off a group of one.** The same version grouped the children with
`chunks(MAX_BRANCHING)`, which can produce a group with a single child. Such
a group is a new level that holds one child: it adds depth without adding
capacity. After 2000 inserts at the same position the tree had a depth of 233
instead of 4. The fix is to split a node in the middle, so that every group
has at least two children. The lesson: **a tree that only grows at one end
needs a split rule that does not create empty levels.**

**An absolute offset where a relative one was needed.** The third bug was
found by the benchmark of week 9, not by a test: the check for a character
boundary walked the tree with an absolute offset, while the search for the
child expected an offset relative to the node. On a tree with one chunk the
two numbers are the same, so every unit test passed. As soon as the text was
bigger than one chunk, the check looked in the wrong chunk and reported a
valid offset as "not on a character boundary". The lesson: **a test that only
uses one shape of input tests one shape of the data.** The regression test
that now covers it builds a tree with several levels and checks the chunk
edges.

All three bugs are the kind that only a test with a structural assertion
finds, or a benchmark that runs with realistic sizes. In the first two cases
the text was correct and only the structure or the speed was wrong; in the
third case the text was right and a valid edit was rejected. The lesson for
the prototype: **a data structure needs tests on the shape of the structure,
not only on the result.**

## Consequences

- The core has two storage backends that behave the same, and the document
  level rules (revision, metadata, transactions, errors) are proven to be
  backend independent: the same transaction tests run against both.
- The rope answers the week 8 requirements R3, R5, R6 and R9 structurally:
  an edit touches the chunks around the edit, a range read touches the chunks
  of the range, and a snapshot is a copy of the tree, not of the text.
- The prototype pays for that with memory: every chunk carries a `String`
  header and every internal node carries a vector. The overhead is measured
  in week 10, not estimated here.
- The prototype is **not** a production backend. The known limits are listed
  below, and week 10 has to decide whether to fix them or to keep the string
  backend as the default for large documents.

## Known limits of the prototype

| Limit | Consequence | Fix in a production version |
|---|---|---|
| `split_at` and `concat` are O(n) | not usable on the hot path | split the node path, merge the trees |
| Deleted chunks are not merged | the tree keeps nodes that are underfull | merge underfull nodes with their sibling |
| The tree is not perfectly balanced | the depth can exceed the optimum | rebuild a level when it overflows, ropey style |
| The chunk size is fixed at 1 KiB | not tuned for the workload | make it configurable per document |
| No snapshot support | a background task cannot hold a stable view | share the nodes with `Rc`/`Arc` |
| Text is copied per chunk | a snapshot costs the whole text | share the nodes between snapshots |

## Lifecycle

| Item | Lifecycle | Notes |
|---|---|---|
| `Rope` | Experimental | prototype, not a default backend |
| `RopeTextStore` | Experimental | second implementation of the trait |
| `Document<S>` | Stable | the generic parameter is what makes the second backend usable |
| `StringTextStore` | Stable | stays the default until week 10 decides otherwise |

## Future

- Week 10 measures the two backends against the scenarios and thresholds of
  `docs/benchmark/benchmark-requirements.md`, and publishes
  `docs/benchmark/rope-week10.md`.
- Week 10 also decides how the public API offers the choice: generic
  parameter, `Box<dyn TextStore>` or an explicit enum. The generic parameter
  is in place; the public shape is not decided.
- Week 12 (long line) uses the chunk size rule of this prototype: the line
  index of a 100 MB single line has to work on chunks, not on one string.
- A production rope needs shared nodes for snapshots (R6): the background
  task of week 18 keeps a snapshot of the text while the user keeps typing.