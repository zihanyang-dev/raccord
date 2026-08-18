#![forbid(unsafe_code)]

use raccord_media::ArtifactRef;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CacheKey(String);

impl CacheKey {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.is_empty()).then_some(Self(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheEntry {
    pub key: CacheKey,
    pub artifact: ArtifactRef,
}
