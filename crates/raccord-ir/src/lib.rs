#![forbid(unsafe_code)]

use raccord_time::{FrameRange, FrameRate};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProjectId(String);

impl ProjectId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.is_empty()).then_some(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ClipId(String);

impl ClipId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.is_empty()).then_some(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SourceRef(String);

impl SourceRef {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.is_empty()).then_some(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Clip {
    id: ClipId,
    source: SourceRef,
    timeline_range: FrameRange,
}

impl Clip {
    pub fn new(id: ClipId, source: SourceRef, timeline_range: FrameRange) -> Self {
        Self {
            id,
            source,
            timeline_range,
        }
    }

    pub fn id(&self) -> &ClipId {
        &self.id
    }

    pub fn source(&self) -> &SourceRef {
        &self.source
    }

    pub const fn timeline_range(&self) -> FrameRange {
        self.timeline_range
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Composition {
    frame_rate: FrameRate,
    clips: Vec<Clip>,
}

impl Composition {
    pub fn new(frame_rate: FrameRate) -> Self {
        Self {
            frame_rate,
            clips: Vec::new(),
        }
    }

    pub const fn frame_rate(&self) -> FrameRate {
        self.frame_rate
    }

    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    pub fn add_clip(&mut self, clip: Clip) {
        self.clips.push(clip);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    id: ProjectId,
    root: Composition,
}

impl Project {
    pub fn new(id: ProjectId, root: Composition) -> Self {
        Self { id, root }
    }

    pub fn id(&self) -> &ProjectId {
        &self.id
    }

    pub fn root(&self) -> &Composition {
        &self.root
    }
}
