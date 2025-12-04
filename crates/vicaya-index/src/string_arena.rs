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
    fn test_string_arena_basic() {
        let mut arena = StringArena::new();

        let (off1, len1) = arena.add("hello");
        let (off2, len2) = arena.add("world");

        assert_eq!(arena.get(off1, len1), Some("hello"));
        assert_eq!(arena.get(off2, len2), Some("world"));
    }

    #[test]
    fn test_empty_arena() {
        let arena = StringArena::new();
        assert_eq!(arena.size(), 0);
        assert_eq!(arena.get(0, 0), Some(""));
    }

    #[test]
    fn test_empty_string() {
        let mut arena = StringArena::new();
        let (offset, len) = arena.add("");
        assert_eq!(offset, 0);
        assert_eq!(len, 0);
        assert_eq!(arena.get(offset, len), Some(""));
    }

    #[test]
    fn test_unicode() {
        let mut arena = StringArena::new();
        let unicode_str = "Hello 世界 🌍";
        let (offset, len) = arena.add(unicode_str);
        assert_eq!(arena.get(offset, len), Some(unicode_str));
    }

    #[test]
    fn test_sequential_adds() {
        let mut arena = StringArena::new();

        let (off1, len1) = arena.add("first");
        assert_eq!(off1, 0);
        assert_eq!(arena.size(), 5);

        let (off2, len2) = arena.add("second");
        assert_eq!(off2, 5);
        assert_eq!(arena.size(), 11);

        let (off3, len3) = arena.add("third");
        assert_eq!(off3, 11);
        assert_eq!(arena.size(), 16);

        assert_eq!(arena.get(off1, len1), Some("first"));
        assert_eq!(arena.get(off2, len2), Some("second"));
        assert_eq!(arena.get(off3, len3), Some("third"));
    }

    #[test]
    fn test_invalid_range() {
        let arena = StringArena::new();
        assert_eq!(arena.get(0, 100), None); // Out of bounds
        assert_eq!(arena.get(100, 1), None); // Out of bounds
    }

    #[test]
    fn test_invalid_offset_length() {
        let mut arena = StringArena::new();
        arena.add("hello");

        // Invalid length
        assert_eq!(arena.get(0, 100), None);

        // Invalid offset
        assert_eq!(arena.get(100, 5), None);
    }

    #[test]
    fn test_overlapping_reads() {
        let mut arena = StringArena::new();
        arena.add("abcdef");

        assert_eq!(arena.get(0, 3), Some("abc"));
        assert_eq!(arena.get(3, 3), Some("def"));
        assert_eq!(arena.get(1, 4), Some("bcde"));
    }

    #[test]
    fn test_default() {
        let arena = StringArena::default();
        assert_eq!(arena.size(), 0);
    }

    #[test]
    fn test_long_strings() {
        let mut arena = StringArena::new();
        let long_str = "a".repeat(10000);
        let (offset, len) = arena.add(&long_str);
        assert_eq!(len, 10000);
        assert_eq!(arena.get(offset, len), Some(long_str.as_str()));
    }
}
