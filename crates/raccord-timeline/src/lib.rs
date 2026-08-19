#![forbid(unsafe_code)]

use core::fmt;

use raccord_ir::{ClipId, Project};
use raccord_time::{FrameCount, FrameIndex, FrameRange};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Anchor {
    SequenceStart,
    SequenceEnd,
    ClipStart(ClipId),
    ClipEnd(ClipId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransitionKind {
    Crossfade,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransitionError {
    EmptyId,
    ZeroDuration,
    MissingClip(ClipId),
    NonAdjacent {
        from: ClipId,
        to: ClipId,
    },
    DurationExceedsClip {
        duration: FrameCount,
        maximum: FrameCount,
    },
}

impl fmt::Display for TransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId => write!(formatter, "transition id must not be empty"),
            Self::ZeroDuration => write!(formatter, "transition duration must be non-zero"),
            Self::MissingClip(clip) => {
                write!(formatter, "transition clip is missing: {}", clip.as_str())
            }
            Self::NonAdjacent { from, to } => write!(
                formatter,
                "transition clips are not adjacent: {} -> {}",
                from.as_str(),
                to.as_str()
            ),
            Self::DurationExceedsClip { duration, maximum } => write!(
                formatter,
                "transition duration {} exceeds clip limit {}",
                duration.value(),
                maximum.value()
            ),
        }
    }
}

impl std::error::Error for TransitionError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transition {
    id: String,
    from_clip: ClipId,
    to_clip: ClipId,
    kind: TransitionKind,
    duration: FrameCount,
}

impl Transition {
    pub fn new(
        id: impl Into<String>,
        from_clip: ClipId,
        to_clip: ClipId,
        kind: TransitionKind,
        duration: FrameCount,
    ) -> Result<Self, TransitionError> {
        let id = id.into();
        if id.is_empty() {
            return Err(TransitionError::EmptyId);
        }
        if duration.is_zero() {
            return Err(TransitionError::ZeroDuration);
        }
        Ok(Self {
            id,
            from_clip,
            to_clip,
            kind,
            duration,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn from_clip(&self) -> &ClipId {
        &self.from_clip
    }

    pub fn to_clip(&self) -> &ClipId {
        &self.to_clip
    }

    pub const fn kind(&self) -> &TransitionKind {
        &self.kind
    }

    pub const fn duration(&self) -> FrameCount {
        self.duration
    }

    pub fn validate(&self, project: &Project) -> Result<(), TransitionError> {
        let clips = project.root().clips();
        let from_index = clips
            .iter()
            .position(|clip| clip.id() == &self.from_clip)
            .ok_or_else(|| TransitionError::MissingClip(self.from_clip.clone()))?;
        let to_index = clips
            .iter()
            .position(|clip| clip.id() == &self.to_clip)
            .ok_or_else(|| TransitionError::MissingClip(self.to_clip.clone()))?;
        if from_index + 1 != to_index {
            return Err(TransitionError::NonAdjacent {
                from: self.from_clip.clone(),
                to: self.to_clip.clone(),
            });
        }

        let maximum = clips[from_index]
            .timeline_range()
            .duration()
            .min(clips[to_index].timeline_range().duration());
        if self.duration > maximum {
            return Err(TransitionError::DurationExceedsClip {
                duration: self.duration,
                maximum,
            });
        }
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use raccord_ir::{Clip, Composition, Project, ProjectId, SourceRef};
    use raccord_time::{FrameCount, FrameIndex, FrameRate};

    use super::{Transition, TransitionError, TransitionKind};

    fn project() -> Project {
        let mut composition = Composition::new(FrameRate::new(24, 1).expect("valid frame rate"));
        for (id, start, duration) in [("a", 0, 48), ("b", 48, 72), ("c", 120, 48)] {
            composition.add_clip(Clip::new(
                raccord_ir::ClipId::new(id).expect("non-empty clip id"),
                SourceRef::new(format!("asset://{id}")).expect("non-empty source"),
                raccord_time::FrameRange::new(FrameIndex::new(start), FrameCount::new(duration)),
            ));
        }
        Project::new(
            ProjectId::new("fixture").expect("non-empty project id"),
            composition,
        )
    }

    #[test]
    fn adjacent_transition_validates_against_clip_durations() {
        let transition = Transition::new(
            "crossfade-a-b",
            raccord_ir::ClipId::new("a").expect("valid id"),
            raccord_ir::ClipId::new("b").expect("valid id"),
            TransitionKind::Crossfade,
            FrameCount::new(24),
        )
        .expect("transition is well formed");

        assert_eq!(transition.validate(&project()), Ok(()));
    }

    #[test]
    fn non_adjacent_transition_is_rejected() {
        let transition = Transition::new(
            "crossfade-a-c",
            raccord_ir::ClipId::new("a").expect("valid id"),
            raccord_ir::ClipId::new("c").expect("valid id"),
            TransitionKind::Crossfade,
            FrameCount::new(24),
        )
        .expect("transition is well formed");

        assert!(matches!(
            transition.validate(&project()),
            Err(TransitionError::NonAdjacent { .. })
        ));
    }

    #[test]
    fn transition_cannot_exceed_the_shorter_clip() {
        let transition = Transition::new(
            "crossfade-a-b",
            raccord_ir::ClipId::new("a").expect("valid id"),
            raccord_ir::ClipId::new("b").expect("valid id"),
            TransitionKind::Crossfade,
            FrameCount::new(49),
        )
        .expect("transition is well formed");

        assert!(matches!(
            transition.validate(&project()),
            Err(TransitionError::DurationExceedsClip { duration, maximum })
                if duration == FrameCount::new(49) && maximum == FrameCount::new(48)
        ));
    }
}
