# ADR-0001: TextOffset Representation

## Status

Accepted temporarily.

Superseded by ADR-0004 for the position model. Byte offsets stay canonical; the
other coordinate systems (scalar, grapheme, line/column) are named but deferred.
See `docs/decisions/0004-text-position-model.md` and
`docs/design/text-position.md`.

## Decision

TextOffset currently represents a UTF-8 byte offset.

## Reason

The initial implementation uses Rust String, and Rust String APIs use byte indices.

## Future

We need to investigate:

- Unicode scalar values
- Grapheme clusters
- Line and column positions
- Visual positions
- Rope-based storage

The public API should not permanently depend on the internal text storage implementation.
