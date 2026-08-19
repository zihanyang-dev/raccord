use std::collections::BTreeMap;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::Timeline;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheKeys {
    pub media_render_key: String,
    pub subtitle_overlay_key: String,
    pub metadata_key: String,
    pub transition_render_key: String,
    pub clip_media_render_keys: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CacheKeyError {
    MissingMediaHash(String),
    Serialization(String),
}

pub fn cache_keys(
    timeline: &Timeline,
    media_hashes: &BTreeMap<String, String>,
) -> Result<CacheKeys, CacheKeyError> {
    let mut clips = Vec::with_capacity(timeline.clips.len());
    let mut clip_media_render_keys = BTreeMap::new();
    for clip in &timeline.clips {
        let asset = clip
            .source
            .strip_prefix("asset://")
            .unwrap_or(clip.source.as_str());
        let media_sha256 = media_hashes
            .get(asset)
            .ok_or_else(|| CacheKeyError::MissingMediaHash(asset.to_owned()))?;
        let material = json!({
            "audio_gain_db_milli": clip.audio_gain_db_milli,
            "duration_frames": clip.duration_frames,
            "id": clip.id,
            "media_sha256": media_sha256,
            "source": clip.source,
        });
        clip_media_render_keys.insert(clip.id.clone(), hash_json(&material)?);
        clips.push(material);
    }
    let placements = serde_json::to_value(timeline.placements())
        .map_err(|error| CacheKeyError::Serialization(error.to_string()))?;
    let subtitles = serde_json::to_value(&timeline.subtitles)
        .map_err(|error| CacheKeyError::Serialization(error.to_string()))?;
    let markers = serde_json::to_value(&timeline.markers)
        .map_err(|error| CacheKeyError::Serialization(error.to_string()))?;
    let transitions = serde_json::to_value(&timeline.transitions)
        .map_err(|error| CacheKeyError::Serialization(error.to_string()))?;

    let transition_render_key = hash_json(&json!({
        "placements": placements,
        "transitions": transitions,
    }))?;

    Ok(CacheKeys {
        media_render_key: hash_json(&json!({
            "clips": clips,
            "placements": placements,
            "transitions": transitions,
        }))?,
        subtitle_overlay_key: hash_json(&json!({
            "placements": placements,
            "subtitles": subtitles,
        }))?,
        metadata_key: hash_json(&json!({
            "markers": markers,
            "subtitles": subtitles,
        }))?,
        transition_render_key,
        clip_media_render_keys,
    })
}

fn hash_json(value: &serde_json::Value) -> Result<String, CacheKeyError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| CacheKeyError::Serialization(error.to_string()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::cache_keys;
    use crate::{SemanticEdit, Transaction, demo_timeline, plan_edit};
    use std::collections::BTreeMap;

    fn media_hashes() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("a".into(), "hash-a".into()),
            ("b".into(), "hash-b".into()),
            ("c".into(), "hash-c".into()),
        ])
    }

    fn state(edits: Vec<SemanticEdit>) -> crate::Timeline {
        let preview = plan_edit(
            &demo_timeline(),
            &Transaction {
                base_revision: 0,
                edits,
            },
        )
        .expect("cache fixture is valid");
        preview.timeline
    }

    #[test]
    fn marker_only_changes_reuse_render_keys() {
        let base = state(vec![
            SemanticEdit::SetAudioGain {
                clip_id: "b".into(),
                gain_db_milli: -3000,
            },
            SemanticEdit::AddSubtitle {
                id: "subtitle-1".into(),
                clip_id: "b".into(),
                text: "Hello".into(),
            },
        ]);
        let marker = state(vec![
            SemanticEdit::SetAudioGain {
                clip_id: "b".into(),
                gain_db_milli: -3000,
            },
            SemanticEdit::AddSubtitle {
                id: "subtitle-1".into(),
                clip_id: "b".into(),
                text: "Hello".into(),
            },
            SemanticEdit::AddMarker {
                id: "marker-1".into(),
                clip_id: "c".into(),
                label: "end beat".into(),
            },
        ]);
        let base_keys = cache_keys(&base, &media_hashes()).expect("base keys are valid");
        let marker_keys = cache_keys(&marker, &media_hashes()).expect("marker keys are valid");

        assert_eq!(base_keys.media_render_key, marker_keys.media_render_key);
        assert_eq!(
            base_keys.clip_media_render_keys,
            marker_keys.clip_media_render_keys
        );
        assert_eq!(
            base_keys.subtitle_overlay_key,
            marker_keys.subtitle_overlay_key
        );
        assert_ne!(base_keys.metadata_key, marker_keys.metadata_key);
    }

    #[test]
    fn transition_changes_composite_keys_but_not_clip_keys() {
        let base = state(Vec::new());
        let transitioned = state(vec![SemanticEdit::AddTransition {
            id: "crossfade-b-c".into(),
            from_clip_id: "b".into(),
            to_clip_id: "c".into(),
            kind: "crossfade".into(),
            duration_frames: 24,
        }]);
        let base_keys = cache_keys(&base, &media_hashes()).expect("base keys are valid");
        let transition_keys =
            cache_keys(&transitioned, &media_hashes()).expect("transition keys are valid");

        assert_ne!(base_keys.media_render_key, transition_keys.media_render_key);
        assert_ne!(
            base_keys.transition_render_key,
            transition_keys.transition_render_key
        );
        assert_eq!(
            base_keys.clip_media_render_keys,
            transition_keys.clip_media_render_keys
        );
        assert_eq!(
            base_keys.subtitle_overlay_key,
            transition_keys.subtitle_overlay_key
        );
    }

    #[test]
    fn gain_changes_media_key_but_not_subtitle_key() {
        let original = state(vec![SemanticEdit::SetAudioGain {
            clip_id: "b".into(),
            gain_db_milli: -3000,
        }]);
        let changed = state(vec![SemanticEdit::SetAudioGain {
            clip_id: "b".into(),
            gain_db_milli: -6000,
        }]);
        let original_keys =
            cache_keys(&original, &media_hashes()).expect("original keys are valid");
        let changed_keys = cache_keys(&changed, &media_hashes()).expect("changed keys are valid");

        assert_ne!(
            original_keys.media_render_key,
            changed_keys.media_render_key
        );
        assert_eq!(
            original_keys.subtitle_overlay_key,
            changed_keys.subtitle_overlay_key
        );
        assert_ne!(
            original_keys.clip_media_render_keys["b"],
            changed_keys.clip_media_render_keys["b"]
        );
    }
}
