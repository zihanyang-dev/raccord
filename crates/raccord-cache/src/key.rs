use sha2::{Digest, Sha256};

/// Stable content-addressed key for a rendered artifact.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CacheKey(String);

impl CacheKey {
    /// Create a key from a path-safe serialized value.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        let valid = !value.is_empty()
            && value != "."
            && value != ".."
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte));
        valid.then_some(Self(value))
    }

    /// Build a deterministic key from a namespace and ordered semantic parts.
    pub fn from_parts(namespace: &str, parts: &[&str]) -> Option<Self> {
        if CacheKey::new(namespace).is_none() || parts.iter().any(|part| part.is_empty()) {
            return None;
        }

        let mut hasher = Sha256::new();
        hasher.update(namespace.as_bytes());
        hasher.update([0]);
        for part in parts {
            hasher.update(part.as_bytes());
            hasher.update([0]);
        }
        let digest = hasher.finalize();
        let short_digest: String = digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        CacheKey::new(format!("{namespace}-{short_digest}"))
    }

    /// Return the serialized cache key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
