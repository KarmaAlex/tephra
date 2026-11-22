//! obsidilight-core: platform-agnostic pieces with no GUI dependencies.
pub mod buffer;
pub mod file;
pub mod links;
pub mod markdown;

pub use buffer::{Document, EditKind, Selection};
