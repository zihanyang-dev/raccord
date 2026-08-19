use std::collections::BTreeMap;

use raccord_media::ArtifactRef;
use serde::{Deserialize, Serialize};

use crate::CacheKey;

/// Persisted facts about an artifact, kept separate from the semantic key.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactMetadata {
    /// Number of bytes in the published artifact.
    pub byte_len: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheEntry {
    /// Semantic key used to address the artifact.
    pub key: CacheKey,
    /// Renderer-produced artifact reference.
    pub artifact: ArtifactRef,
    /// Persisted artifact facts used for validation.
    pub metadata: ArtifactMetadata,
}

/// In-memory index used by cache planners before persistence is selected.
#[derive(Default)]
pub struct CacheIndex {
    entries: BTreeMap<CacheKey, CacheEntry>,
}

impl CacheIndex {
    /// Insert an entry, returning the previous entry for the same key.
    pub fn insert(&mut self, entry: CacheEntry) -> Option<CacheEntry> {
        self.entries.insert(entry.key.clone(), entry)
    }

    /// Look up an entry without changing the index.
    pub fn get(&self, key: &CacheKey) -> Option<&CacheEntry> {
        self.entries.get(key)
    }

    /// Return the number of indexed entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return whether the index contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
