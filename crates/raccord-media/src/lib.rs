#![forbid(unsafe_code)]

use raccord_time::{FrameCount, FrameRate};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaKind {
    Video,
    Audio,
    Image,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactRef {
    digest: String,
}

impl ArtifactRef {
    pub fn new(digest: impl Into<String>) -> Option<Self> {
        let digest = digest.into();
        (!digest.is_empty()).then_some(Self { digest })
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MediaDescriptor {
    pub kind: MediaKind,
    pub duration: FrameCount,
    pub frame_rate: Option<FrameRate>,
}
