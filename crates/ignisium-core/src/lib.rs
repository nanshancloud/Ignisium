mod document;
mod error;
mod text;

pub use document::Document;
pub use error::{TextError, TextResult};
pub use text::{StringTextStore, TextOffset, TextRange, TextStore};
