# ADR-0001: TextOffset Representation

## Status

Accepted temporarily.

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
