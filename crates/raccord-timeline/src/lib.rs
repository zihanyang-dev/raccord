#![forbid(unsafe_code)]

use raccord_ir::{ClipId, Project};
use raccord_time::{FrameIndex, FrameRange};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Anchor {
    SequenceStart,
    SequenceEnd,
    ClipStart(ClipId),
    ClipEnd(ClipId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Placement {
    clip: ClipId,
    range: FrameRange,
}

impl Placement {
    pub fn new(clip: ClipId, range: FrameRange) -> Self {
        Self { clip, range }
    }

    pub fn clip(&self) -> &ClipId {
        &self.clip
    }

    pub const fn range(&self) -> FrameRange {
        self.range
    }
}

#[derive(Default)]
pub struct Resolver;

impl Resolver {
    pub fn resolve(&self, project: &Project) -> Vec<Placement> {
        project
            .root()
            .clips()
            .iter()
            .map(|clip| Placement::new(clip.id().clone(), clip.timeline_range()))
            .collect()
    }

    pub fn resolve_anchor(&self, anchor: &Anchor) -> Option<FrameIndex> {
        match anchor {
            Anchor::SequenceStart => Some(FrameIndex::ZERO),
            Anchor::SequenceEnd | Anchor::ClipStart(_) | Anchor::ClipEnd(_) => None,
        }
    }
}
