#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub mod cache;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub source: String,
    pub duration_frames: u64,
    #[serde(default)]
    pub audio_gain_db_milli: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub id: String,
    pub clip_id: String,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SubtitleCue {
    pub id: String,
    pub clip_id: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub id: String,
    pub from_clip_id: String,
    pub to_clip_id: String,
    pub kind: String,
    pub duration_frames: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub id: String,
    pub start_frame: u64,
    pub duration_frames: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub revision: u64,
    pub clips: Vec<Clip>,
    pub markers: Vec<Marker>,
    pub subtitles: Vec<SubtitleCue>,
    #[serde(default)]
    pub transitions: Vec<Transition>,
}

impl Timeline {
    pub fn placements(&self) -> Vec<Placement> {
        let mut cursor = 0;

        self.clips
            .iter()
            .map(|clip| {
                let placement = Placement {
                    id: clip.id.clone(),
                    start_frame: cursor,
                    duration_frames: clip.duration_frames,
                };
                cursor += clip.duration_frames;
                placement
            })
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum SemanticEdit {
    RippleDelete {
        clip_id: String,
    },
    ReplaceSource {
        clip_id: String,
        source: String,
    },
    InsertAfter {
        clip: Clip,
        after: Option<String>,
    },
    MoveAfter {
        clip_id: String,
        after: Option<String>,
    },
    Trim {
        clip_id: String,
        duration_frames: u64,
    },
    SetAudioGain {
        clip_id: String,
        gain_db_milli: i32,
    },
    AddMarker {
        id: String,
        clip_id: String,
        label: String,
    },
    AddSubtitle {
        id: String,
        clip_id: String,
        text: String,
    },
    AddTransition {
        id: String,
        from_clip_id: String,
        to_clip_id: String,
        kind: String,
        duration_frames: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    pub base_revision: u64,
    pub edits: Vec<SemanticEdit>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Preview {
    pub timeline: Timeline,
    pub changed_clip_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EditError {
    StaleRevision { expected: u64, actual: u64 },
    MissingClip(String),
    DuplicateClip(String),
    MissingAnchor(String),
    DuplicateMetadata(String),
    InvalidDuration,
    EmptyMetadata,
    DuplicateTransition(String),
    InvalidTransition,
}

pub fn demo_timeline() -> Timeline {
    Timeline {
        revision: 0,
        clips: vec![
            Clip {
                id: "a".into(),
                source: "asset://a".into(),
                duration_frames: 72,
                audio_gain_db_milli: 0,
            },
            Clip {
                id: "b".into(),
                source: "asset://b".into(),
                duration_frames: 72,
                audio_gain_db_milli: 0,
            },
            Clip {
                id: "c".into(),
                source: "asset://c".into(),
                duration_frames: 96,
                audio_gain_db_milli: 0,
            },
        ],
        markers: Vec::new(),
        subtitles: Vec::new(),
        transitions: Vec::new(),
    }
}

pub fn plan_edit(timeline: &Timeline, transaction: &Transaction) -> Result<Preview, EditError> {
    if timeline.revision != transaction.base_revision {
        return Err(EditError::StaleRevision {
            expected: transaction.base_revision,
            actual: timeline.revision,
        });
    }

    let mut next = timeline.clone();
    let mut changed_clip_ids = Vec::new();

    for edit in &transaction.edits {
        apply_edit(&mut next, edit, &mut changed_clip_ids)?;
    }

    next.revision += 1;

    Ok(Preview {
        timeline: next,
        changed_clip_ids,
    })
}

pub fn verify(timeline: &Timeline) -> bool {
    timeline.revision > 0
        && timeline.clips.iter().all(|clip| clip.duration_frames > 0)
        && timeline.clips.iter().enumerate().all(|(index, clip)| {
            timeline.clips[index + 1..]
                .iter()
                .all(|other| other.id != clip.id)
        })
        && timeline.markers.iter().all(|marker| {
            !marker.id.is_empty()
                && !marker.label.is_empty()
                && timeline.clips.iter().any(|clip| clip.id == marker.clip_id)
        })
        && timeline.subtitles.iter().all(|subtitle| {
            !subtitle.id.is_empty()
                && !subtitle.text.is_empty()
                && timeline
                    .clips
                    .iter()
                    .any(|clip| clip.id == subtitle.clip_id)
        })
        && timeline.transitions.iter().all(|transition| {
            !transition.id.is_empty()
                && transition.kind == "crossfade"
                && transition.duration_frames > 0
                && timeline.clips.windows(2).any(|window| {
                    window[0].id == transition.from_clip_id && window[1].id == transition.to_clip_id
                })
                && transition.duration_frames
                    <= timeline
                        .clips
                        .iter()
                        .find(|clip| clip.id == transition.from_clip_id)
                        .map_or(0, |clip| clip.duration_frames)
        })
        && metadata_ids_are_unique(timeline)
}

fn metadata_ids_are_unique(timeline: &Timeline) -> bool {
    let ids = timeline.markers.iter().map(|marker| marker.id.as_str());
    let marker_ids_are_unique = ids
        .clone()
        .enumerate()
        .all(|(index, id)| ids.clone().skip(index + 1).all(|other| other != id));
    let subtitle_ids_are_unique = timeline
        .subtitles
        .iter()
        .enumerate()
        .all(|(index, subtitle)| {
            timeline.subtitles[index + 1..]
                .iter()
                .all(|other| other.id != subtitle.id)
        });
    let cross_namespace_ids_are_unique = timeline.markers.iter().all(|marker| {
        timeline
            .subtitles
            .iter()
            .all(|subtitle| subtitle.id != marker.id)
    });
    let transition_ids_are_unique =
        timeline
            .transitions
            .iter()
            .enumerate()
            .all(|(index, transition)| {
                timeline.transitions[index + 1..]
                    .iter()
                    .all(|other| other.id != transition.id)
            });
    let transition_metadata_ids_are_unique = timeline.transitions.iter().all(|transition| {
        timeline
            .markers
            .iter()
            .all(|marker| marker.id != transition.id)
            && timeline
                .subtitles
                .iter()
                .all(|subtitle| subtitle.id != transition.id)
    });
    marker_ids_are_unique
        && subtitle_ids_are_unique
        && cross_namespace_ids_are_unique
        && transition_ids_are_unique
        && transition_metadata_ids_are_unique
}

fn apply_edit(
    timeline: &mut Timeline,
    edit: &SemanticEdit,
    changed_clip_ids: &mut Vec<String>,
) -> Result<(), EditError> {
    match edit {
        SemanticEdit::RippleDelete { clip_id } => {
            let index = find_clip(&timeline.clips, clip_id)?;
            timeline.clips.remove(index);
            timeline.markers.retain(|marker| marker.clip_id != *clip_id);
            timeline
                .subtitles
                .retain(|subtitle| subtitle.clip_id != *clip_id);
            timeline.transitions.retain(|transition| {
                transition.from_clip_id != *clip_id && transition.to_clip_id != *clip_id
            });
            changed_clip_ids.push(clip_id.clone());
        }
        SemanticEdit::ReplaceSource { clip_id, source } => {
            let clip = timeline
                .clips
                .iter_mut()
                .find(|clip| clip.id == *clip_id)
                .ok_or_else(|| EditError::MissingClip(clip_id.clone()))?;
            clip.source = source.clone();
            changed_clip_ids.push(clip_id.clone());
        }
        SemanticEdit::InsertAfter { clip, after } => {
            if timeline
                .clips
                .iter()
                .any(|candidate| candidate.id == clip.id)
            {
                return Err(EditError::DuplicateClip(clip.id.clone()));
            }
            if clip.duration_frames == 0 {
                return Err(EditError::InvalidDuration);
            }

            let index = match after {
                Some(anchor) => find_clip(&timeline.clips, anchor)? + 1,
                None => 0,
            };
            changed_clip_ids.push(clip.id.clone());
            timeline.clips.insert(index, clip.clone());
        }
        SemanticEdit::MoveAfter { clip_id, after } => {
            let current_index = find_clip(&timeline.clips, clip_id)?;
            let clip = timeline.clips.remove(current_index);
            let destination = match after {
                Some(anchor) if anchor == clip_id => {
                    return Err(EditError::MissingAnchor(anchor.clone()));
                }
                Some(anchor) => find_clip(&timeline.clips, anchor)? + 1,
                None => 0,
            };
            timeline.clips.insert(destination, clip);
            changed_clip_ids.push(clip_id.clone());
        }
        SemanticEdit::Trim {
            clip_id,
            duration_frames,
        } => {
            if *duration_frames == 0 {
                return Err(EditError::InvalidDuration);
            }
            let clip = timeline
                .clips
                .iter_mut()
                .find(|clip| clip.id == *clip_id)
                .ok_or_else(|| EditError::MissingClip(clip_id.clone()))?;
            clip.duration_frames = *duration_frames;
            changed_clip_ids.push(clip_id.clone());
        }
        SemanticEdit::SetAudioGain {
            clip_id,
            gain_db_milli,
        } => {
            let clip = timeline
                .clips
                .iter_mut()
                .find(|clip| clip.id == *clip_id)
                .ok_or_else(|| EditError::MissingClip(clip_id.clone()))?;
            clip.audio_gain_db_milli = *gain_db_milli;
            changed_clip_ids.push(clip_id.clone());
        }
        SemanticEdit::AddMarker { id, clip_id, label } => {
            find_clip(&timeline.clips, clip_id)?;
            if id.is_empty() || label.is_empty() {
                return Err(EditError::EmptyMetadata);
            }
            if metadata_id_exists(timeline, id) {
                return Err(EditError::DuplicateMetadata(id.clone()));
            }
            timeline.markers.push(Marker {
                id: id.clone(),
                clip_id: clip_id.clone(),
                label: label.clone(),
            });
            changed_clip_ids.push(clip_id.clone());
        }
        SemanticEdit::AddSubtitle { id, clip_id, text } => {
            find_clip(&timeline.clips, clip_id)?;
            if id.is_empty() || text.is_empty() {
                return Err(EditError::EmptyMetadata);
            }
            if metadata_id_exists(timeline, id) {
                return Err(EditError::DuplicateMetadata(id.clone()));
            }
            timeline.subtitles.push(SubtitleCue {
                id: id.clone(),
                clip_id: clip_id.clone(),
                text: text.clone(),
            });
            changed_clip_ids.push(clip_id.clone());
        }
        SemanticEdit::AddTransition {
            id,
            from_clip_id,
            to_clip_id,
            kind,
            duration_frames,
        } => {
            if id.is_empty() || kind != "crossfade" || from_clip_id == to_clip_id {
                return Err(EditError::InvalidTransition);
            }
            if *duration_frames == 0 {
                return Err(EditError::InvalidDuration);
            }
            if metadata_id_exists(timeline, id) {
                return Err(EditError::DuplicateTransition(id.clone()));
            }
            let from_index = find_clip(&timeline.clips, from_clip_id)?;
            let to_index = find_clip(&timeline.clips, to_clip_id)?;
            if to_index != from_index + 1
                || *duration_frames > timeline.clips[from_index].duration_frames
            {
                return Err(EditError::InvalidTransition);
            }
            timeline.transitions.push(Transition {
                id: id.clone(),
                from_clip_id: from_clip_id.clone(),
                to_clip_id: to_clip_id.clone(),
                kind: kind.clone(),
                duration_frames: *duration_frames,
            });
            changed_clip_ids.extend([from_clip_id.clone(), to_clip_id.clone()]);
        }
    }

    Ok(())
}

fn metadata_id_exists(timeline: &Timeline, id: &str) -> bool {
    timeline.markers.iter().any(|marker| marker.id == id)
        || timeline.subtitles.iter().any(|subtitle| subtitle.id == id)
        || timeline.transitions.iter().any(|transition| transition.id == id)
}

fn find_clip(clips: &[Clip], id: &str) -> Result<usize, EditError> {
    clips
        .iter()
        .position(|clip| clip.id == id)
        .ok_or_else(|| EditError::MissingClip(id.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{EditError, SemanticEdit, Transaction, demo_timeline, plan_edit, verify};

    #[test]
    fn ripple_delete_reflows_the_following_clip() {
        let timeline = demo_timeline();
        let transaction = Transaction {
            base_revision: 0,
            edits: vec![SemanticEdit::RippleDelete {
                clip_id: "b".into(),
            }],
        };

        let preview = plan_edit(&timeline, &transaction).expect("fixture is valid");
        let placements = preview.timeline.placements();

        assert_eq!(placements[0].start_frame, 0);
        assert_eq!(placements[0].duration_frames, 72);
        assert_eq!(placements[1].start_frame, 72);
        assert_eq!(placements[1].duration_frames, 96);
        assert_eq!(preview.changed_clip_ids, vec!["b"]);
        assert_eq!(preview.timeline.revision, 1);
        assert!(verify(&preview.timeline));
    }

    #[test]
    fn metadata_edits_are_anchored_and_verified() {
        let transaction = Transaction {
            base_revision: 0,
            edits: vec![
                SemanticEdit::SetAudioGain {
                    clip_id: "b".into(),
                    gain_db_milli: -3000,
                },
                SemanticEdit::AddMarker {
                    id: "marker-1".into(),
                    clip_id: "b".into(),
                    label: "beat drop".into(),
                },
                SemanticEdit::AddSubtitle {
                    id: "subtitle-1".into(),
                    clip_id: "b".into(),
                    text: "Hello".into(),
                },
            ],
        };

        let preview = plan_edit(&demo_timeline(), &transaction).expect("metadata is valid");
        assert_eq!(preview.timeline.clips[1].audio_gain_db_milli, -3000);
        assert_eq!(preview.timeline.markers.len(), 1);
        assert_eq!(preview.timeline.subtitles.len(), 1);
        assert!(verify(&preview.timeline));

        let delete = Transaction {
            base_revision: 1,
            edits: vec![SemanticEdit::RippleDelete {
                clip_id: "b".into(),
            }],
        };
        let after_delete = plan_edit(&preview.timeline, &delete).expect("delete is valid");
        assert!(after_delete.timeline.markers.is_empty());
        assert!(after_delete.timeline.subtitles.is_empty());
    }

    #[test]
    fn transitions_require_adjacent_clips_and_verify() {
        let transaction = Transaction {
            base_revision: 0,
            edits: vec![SemanticEdit::AddTransition {
                id: "crossfade-b-c".into(),
                from_clip_id: "b".into(),
                to_clip_id: "c".into(),
                kind: "crossfade".into(),
                duration_frames: 24,
            }],
        };

        let preview = plan_edit(&demo_timeline(), &transaction).expect("transition is valid");
        assert_eq!(preview.timeline.transitions.len(), 1);
        assert_eq!(preview.changed_clip_ids, vec!["b", "c"]);
        assert!(verify(&preview.timeline));

        let invalid = Transaction {
            base_revision: 0,
            edits: vec![SemanticEdit::AddTransition {
                id: "invalid".into(),
                from_clip_id: "a".into(),
                to_clip_id: "c".into(),
                kind: "crossfade".into(),
                duration_frames: 24,
            }],
        };
        assert_eq!(
            plan_edit(&demo_timeline(), &invalid),
            Err(EditError::InvalidTransition)
        );
    }

    #[test]
    fn stale_transactions_are_rejected_before_mutation() {
        let transaction = Transaction {
            base_revision: 7,
            edits: Vec::new(),
        };

        assert_eq!(
            plan_edit(&demo_timeline(), &transaction),
            Err(EditError::StaleRevision {
                expected: 7,
                actual: 0,
            })
        );
    }
}
