//! tephra-core: platform-agnostic pieces with no GUI dependencies.
pub mod buffer;
pub mod file;
pub mod index;
pub mod links;
pub mod markdown;
pub mod search;
pub mod sort;

pub use buffer::{Document, EditKind, Selection};
