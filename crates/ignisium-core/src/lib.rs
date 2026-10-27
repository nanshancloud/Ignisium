//! Ignisium core: a platform independent text editing component.
//!
//! Everything a host application needs is re-exported at the crate root; the
//! internal module layout is not part of the public API.

mod document;
mod edit;
mod error;
mod metadata;
mod revision;
mod text;

pub use document::Document;
pub use edit::{EditProblem, TextEdit};
pub use error::{TextError, TextResult};
pub use metadata::DocumentMetadata;
pub use revision::RevisionId;
pub use text::{Rope, RopeTextStore, StringTextStore, TextOffset, TextRange, TextStore};
