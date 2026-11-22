//! Obsidian-flavoured Markdown: blocks, inlines and math ranges, all with source byte ranges.
pub mod blocks;
pub mod callouts;
pub mod inline;
pub mod math;
pub mod tables;

pub use blocks::{parse_blocks, Block, BlockKind};
pub use inline::{parse_inline, Inline, InlineKind};
pub use math::{find_math, MathRange};
pub use callouts::{parse_quote, Callout, QuoteItem};
pub use tables::{parse_table, Align, Table};
