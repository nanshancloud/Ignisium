//! Ignisium core: a platform independent text editing component.
//!
//! The crate currently provides the document model only:
//!
//! ```text
//! Document ──► TextStore ──► StringTextStore ──► String
//! ```
//!
//! Everything a host application needs today is re-exported at the crate root;
//! internal module layout is not part of the public API.

mod document;
mod error;
mod metadata;
mod revision;
mod text;

pub use document::Document;
pub use error::{TextError, TextResult};
pub use metadata::DocumentMetadata;
pub use revision::RevisionId;
pub use text::{StringTextStore, TextOffset, TextRange, TextStore};
