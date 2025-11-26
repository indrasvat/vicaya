//! String arena for efficient storage of file paths.

use serde::{Deserialize, Serialize};

/// Arena for storing strings with minimal overhead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StringArena {
    data: Vec<u8>,
}

impl StringArena {
    /// Create a new empty string arena.
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Add a string to the arena and return its offset and length.
    pub fn add(&mut self, s: &str) -> (usize, usize) {
        let offset = self.data.len();
        let bytes = s.as_bytes();
        self.data.extend_from_slice(bytes);
        (offset, bytes.len())
    }

    /// Get a string from the arena by offset and length.
    pub fn get(&self, offset: usize, len: usize) -> Option<&str> {
        self.data
            .get(offset..offset + len)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
    }

    /// Total size of the arena in bytes.
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

impl Default for StringArena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_arena() {
        let mut arena = StringArena::new();

        let (off1, len1) = arena.add("hello");
        let (off2, len2) = arena.add("world");

        assert_eq!(arena.get(off1, len1), Some("hello"));
        assert_eq!(arena.get(off2, len2), Some("world"));
    }
}
