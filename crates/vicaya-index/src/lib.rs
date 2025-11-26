//! vicaya-index: File table, string arena, trigram index, and query engine.

pub mod file_table;
pub mod query;
pub mod string_arena;
pub mod trigram;

pub use file_table::{FileId, FileMeta, FileTable};
pub use query::{Query, QueryEngine, SearchResult};
pub use string_arena::StringArena;
pub use trigram::{Trigram, TrigramIndex};
