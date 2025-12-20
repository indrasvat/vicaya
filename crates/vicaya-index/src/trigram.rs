//! Trigram index for fast substring search.

use crate::FileId;
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};

/// A trigram: 3 consecutive characters encoded as a u32.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Trigram(pub u32);

impl Trigram {
    /// Create a trigram from three bytes.
    pub fn from_bytes(a: u8, b: u8, c: u8) -> Self {
        Self(((a as u32) << 16) | ((b as u32) << 8) | (c as u32))
    }

    /// Extract trigrams from a string.
    pub fn extract(s: &str) -> Vec<Trigram> {
        let bytes = s.to_lowercase().as_bytes().to_vec();
        if bytes.len() < 3 {
            return Vec::new();
        }

        bytes
            .windows(3)
            .map(|w| Trigram::from_bytes(w[0], w[1], w[2]))
            .collect()
    }
}

/// Trigram inverted index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrigramIndex {
    /// Map from trigram to list of file IDs containing that trigram.
    index: HashMap<Trigram, Vec<FileId>>,
}

impl TrigramIndex {
    /// Create a new empty trigram index.
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
        }
    }

    /// Add a file to the index with its trigrams.
    pub fn add(&mut self, file_id: FileId, text: &str) {
        let trigrams = Trigram::extract(text);
        // Deduplicate trigrams to avoid adding the same file multiple times
        let mut unique_trigrams: Vec<Trigram> = trigrams;
        unique_trigrams.sort_unstable();
        unique_trigrams.dedup();

        for trigram in unique_trigrams {
            self.index.entry(trigram).or_default().push(file_id);
        }
    }

    /// Remove a file from the index.
    pub fn remove(&mut self, file_id: FileId) {
        for posting_list in self.index.values_mut() {
            posting_list.retain(|&id| id != file_id);
        }
    }

    /// Remove a file from only the posting lists implied by the given text.
    ///
    /// This is much cheaper than `remove()` for incremental updates because it
    /// only touches posting lists the file could have been added to.
    pub fn remove_text(&mut self, file_id: FileId, text: &str) {
        let mut trigrams = Trigram::extract(text);
        trigrams.sort_unstable();
        trigrams.dedup();

        let mut maybe_empty = Vec::new();

        for trigram in trigrams {
            if let Some(posting_list) = self.index.get_mut(&trigram) {
                posting_list.retain(|&id| id != file_id);
                if posting_list.is_empty() {
                    maybe_empty.push(trigram);
                }
            }
        }

        for trigram in maybe_empty {
            self.index.remove(&trigram);
        }
    }

    /// Query the index for files containing all given trigrams.
    pub fn query(&self, trigrams: &[Trigram]) -> Vec<FileId> {
        if trigrams.is_empty() {
            return Vec::new();
        }

        // Find the smallest posting list
        let smallest = trigrams
            .iter()
            .filter_map(|t| self.index.get(t))
            .min_by_key(|list| list.len());

        let Some(candidates) = smallest else {
            return Vec::new();
        };

        // Filter candidates that contain all trigrams
        candidates
            .iter()
            .filter(|&&file_id| {
                trigrams.iter().all(|t| {
                    self.index
                        .get(t)
                        .is_some_and(|list| list.contains(&file_id))
                })
            })
            .copied()
            .collect()
    }

    /// Number of unique trigrams in the index.
    pub fn trigram_count(&self) -> usize {
        self.index.len()
    }

    /// Approximate heap bytes used by the trigram index.
    pub fn allocated_bytes(&self) -> usize {
        let entries_bytes = self.index.capacity() * std::mem::size_of::<(Trigram, Vec<FileId>)>();
        let control_bytes = self.index.capacity();
        let postings_bytes: usize = self
            .index
            .values()
            .map(|posting_list| posting_list.capacity() * std::mem::size_of::<FileId>())
            .sum();

        entries_bytes + control_bytes + postings_bytes
    }
}

impl Default for TrigramIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trigram_extract() {
        let trigrams = Trigram::extract("hello");
        assert_eq!(trigrams.len(), 3); // "hel", "ell", "llo"
    }

    #[test]
    fn test_trigram_index() {
        let mut index = TrigramIndex::new();

        index.add(FileId(1), "hello");
        index.add(FileId(2), "world");
        index.add(FileId(3), "hello world");

        let hello_trigrams = Trigram::extract("hel");
        let results = index.query(&hello_trigrams);

        assert!(results.contains(&FileId(1)));
        assert!(results.contains(&FileId(3)));
        assert!(!results.contains(&FileId(2)));
    }
}
