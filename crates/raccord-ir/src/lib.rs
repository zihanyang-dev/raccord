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

    pub fn set_source(&mut self, source: SourceRef) {
        self.source = source;
    }

    pub fn set_timeline_range(&mut self, timeline_range: FrameRange) {
        self.timeline_range = timeline_range;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Marker {
    id: String,
    clip: ClipId,
    label: String,
}

impl Marker {
    pub fn new(id: impl Into<String>, clip: ClipId, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            clip,
            label: label.into(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn clip(&self) -> &ClipId {
        &self.clip
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubtitleCue {
    id: String,
    clip: ClipId,
    text: String,
}

impl SubtitleCue {
    pub fn new(id: impl Into<String>, clip: ClipId, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            clip,
            text: text.into(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn clip(&self) -> &ClipId {
        &self.clip
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Composition {
    frame_rate: FrameRate,
    clips: Vec<Clip>,
    markers: Vec<Marker>,
    subtitles: Vec<SubtitleCue>,
}

impl Composition {
    pub fn new(frame_rate: FrameRate) -> Self {
        Self {
            frame_rate,
            clips: Vec::new(),
            markers: Vec::new(),
            subtitles: Vec::new(),
        }
    }

    pub const fn frame_rate(&self) -> FrameRate {
        self.frame_rate
    }

    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    pub fn subtitles(&self) -> &[SubtitleCue] {
        &self.subtitles
    }

    pub fn add_marker(&mut self, marker: Marker) {
        self.markers.push(marker);
    }

    pub fn add_subtitle(&mut self, subtitle: SubtitleCue) {
        self.subtitles.push(subtitle);
    }

    pub fn add_clip(&mut self, clip: Clip) {
        self.clips.push(clip);
    }

    pub fn remove_clip(&mut self, index: usize) -> Clip {
        self.clips.remove(index)
    }

    pub fn insert_clip(&mut self, index: usize, clip: Clip) -> bool {
        if index > self.clips.len() {
            return false;
        }
        self.clips.insert(index, clip);
        true
    }

    pub fn clips_mut(&mut self) -> &mut [Clip] {
        &mut self.clips
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

    pub fn root_mut(&mut self) -> &mut Composition {
        &mut self.root
    }
}
