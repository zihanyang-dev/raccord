#![forbid(unsafe_code)]

use core::fmt;
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use raccord_ir::{ClipId, Marker, Project, SourceRef, SubtitleCue};
use raccord_planner::{Planner, RenderPlan};
use raccord_rmap::ActionResult;
use raccord_time::{FrameCount, FrameIndex, FrameRange};
use raccord_timeline::{Transition, TransitionError};

static REVISION_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Monotonic project revision used to guard semantic transactions.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Revision(u64);

impl Revision {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    fn next(self) -> Result<Self, TransactionError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(TransactionError::RevisionExhausted)
    }
}

/// One semantic operation carried by a runtime transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticEdit {
    RippleDelete {
        clip: ClipId,
    },
    ReplaceSource {
        clip: ClipId,
        source: SourceRef,
    },
    Trim {
        clip: ClipId,
        duration: FrameCount,
    },
    InsertAfter {
        clip: raccord_ir::Clip,
        after: Option<ClipId>,
    },
    MoveAfter {
        clip: ClipId,
        after: Option<ClipId>,
    },
    AddMarker {
        id: String,
        clip: ClipId,
        label: String,
    },
    AddSubtitle {
        id: String,
        clip: ClipId,
        text: String,
    },
    AddTransition(Transition),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditError {
    MissingClip(ClipId),
    DuplicateClip(ClipId),
    MissingAnchor(ClipId),
    SelfAnchor(ClipId),
    DuplicateMetadata(String),
    EmptyMetadata,
    InvalidDuration,
    ArithmeticOverflow,
    InvalidTransition(TransitionError),
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingClip(clip) => write!(formatter, "clip is missing: {}", clip.as_str()),
            Self::DuplicateClip(clip) => {
                write!(formatter, "clip already exists: {}", clip.as_str())
            }
            Self::MissingAnchor(anchor) => {
                write!(formatter, "anchor clip is missing: {}", anchor.as_str())
            }
            Self::SelfAnchor(clip) => {
                write!(formatter, "clip cannot anchor itself: {}", clip.as_str())
            }
            Self::DuplicateMetadata(id) => write!(formatter, "metadata id already exists: {id}"),
            Self::EmptyMetadata => write!(formatter, "metadata id and content must be non-empty"),
            Self::InvalidDuration => write!(formatter, "edit duration must be non-zero"),
            Self::ArithmeticOverflow => write!(formatter, "edit exceeds the time domain"),
            Self::InvalidTransition(error) => write!(formatter, "invalid transition: {error}"),
        }
    }
}

impl std::error::Error for EditError {}

/// Persistence failures for project revisions.
#[derive(Debug)]
pub enum PersistenceError {
    Io(io::Error),
    InvalidRevision(String),
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "revision persistence I/O error: {error}"),
            Self::InvalidRevision(value) => {
                write!(formatter, "invalid persisted revision: {value}")
            }
        }
    }
}

impl std::error::Error for PersistenceError {}

impl From<io::Error> for PersistenceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Storage boundary for the monotonic project revision.
pub trait RevisionStore {
    fn load(&self) -> Result<Revision, PersistenceError>;
    fn save(&self, revision: Revision) -> Result<(), PersistenceError>;
}

/// Atomic filesystem-backed revision storage.
pub struct FileRevisionStore {
    path: PathBuf,
}

impl FileRevisionStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl RevisionStore for FileRevisionStore {
    fn load(&self) -> Result<Revision, PersistenceError> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Revision::ZERO),
            Err(error) => return Err(error.into()),
        };
        let value = contents
            .trim()
            .parse::<u64>()
            .map_err(|_| PersistenceError::InvalidRevision(contents.trim().to_owned()))?;
        Ok(Revision::new(value))
    }

    fn save(&self, revision: Revision) -> Result<(), PersistenceError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let counter = REVISION_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("revision");
        let temporary = self
            .path
            .with_file_name(format!(".{file_name}.{counter}.tmp"));
        fs::write(&temporary, revision.value().to_string())?;
        fs::rename(temporary, &self.path)?;
        Ok(())
    }
}

/// Errors returned by a commit that also persists its revision.
#[derive(Debug)]
pub enum CommitError {
    Transaction(TransactionError),
    Persistence(PersistenceError),
}

impl fmt::Display for CommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transaction(error) => write!(formatter, "transaction commit failed: {error}"),
            Self::Persistence(error) => {
                write!(formatter, "transaction persistence failed: {error}")
            }
        }
    }
}

impl std::error::Error for CommitError {}

/// Typed semantic edit payload validated before render planning.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EditPayload {
    edits: Vec<SemanticEdit>,
}

impl EditPayload {
    pub fn new(edits: Vec<SemanticEdit>) -> Self {
        Self { edits }
    }

    pub fn edits(&self) -> &[SemanticEdit] {
        &self.edits
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    fn apply(&self, project: &Project) -> Result<Project, TransactionError> {
        let mut edited = project.clone();
        for (index, edit) in self.edits.iter().enumerate() {
            apply_edit(&mut edited, edit)
                .map_err(|source| TransactionError::InvalidEdit { index, source })?;
        }
        Ok(edited)
    }
}

fn find_clip_index(project: &Project, clip: &ClipId) -> Result<usize, EditError> {
    project
        .root()
        .clips()
        .iter()
        .position(|candidate| candidate.id() == clip)
        .ok_or_else(|| EditError::MissingClip(clip.clone()))
}

fn validate_metadata(
    project: &Project,
    id: &str,
    clip: &ClipId,
    content: &str,
) -> Result<(), EditError> {
    if id.is_empty() || content.is_empty() {
        return Err(EditError::EmptyMetadata);
    }
    find_clip_index(project, clip)?;
    if project
        .root()
        .markers()
        .iter()
        .any(|marker| marker.id() == id)
        || project
            .root()
            .subtitles()
            .iter()
            .any(|subtitle| subtitle.id() == id)
    {
        return Err(EditError::DuplicateMetadata(id.to_owned()));
    }
    Ok(())
}

fn reflow(project: &mut Project) -> Result<(), EditError> {
    let mut cursor = 0;
    for clip in project.root_mut().clips_mut() {
        let duration = clip.timeline_range().duration();
        clip.set_timeline_range(FrameRange::new(FrameIndex::new(cursor), duration));
        cursor = cursor
            .checked_add(duration.value())
            .ok_or(EditError::ArithmeticOverflow)?;
    }
    Ok(())
}

fn apply_edit(project: &mut Project, edit: &SemanticEdit) -> Result<(), EditError> {
    match edit {
        SemanticEdit::RippleDelete { clip } => {
            let clip_index = find_clip_index(project, clip)?;
            let duration = project.root().clips()[clip_index]
                .timeline_range()
                .duration();
            project.root_mut().remove_clip(clip_index);
            for following in &mut project.root_mut().clips_mut()[clip_index..] {
                let range = following.timeline_range();
                let start = range
                    .start()
                    .value()
                    .checked_sub(duration.value())
                    .ok_or(EditError::ArithmeticOverflow)?;
                following
                    .set_timeline_range(FrameRange::new(FrameIndex::new(start), range.duration()));
            }
            Ok(())
        }
        SemanticEdit::ReplaceSource { clip, source } => {
            let clip_index = find_clip_index(project, clip)?;
            project.root_mut().clips_mut()[clip_index].set_source(source.clone());
            Ok(())
        }
        SemanticEdit::Trim { clip, duration } => {
            if duration.is_zero() {
                return Err(EditError::InvalidDuration);
            }
            let clip_index = find_clip_index(project, clip)?;
            let range = project.root().clips()[clip_index].timeline_range();
            project.root_mut().clips_mut()[clip_index]
                .set_timeline_range(FrameRange::new(range.start(), *duration));
            Ok(())
        }
        SemanticEdit::InsertAfter { clip, after } => {
            if find_clip_index(project, clip.id()).is_ok() {
                return Err(EditError::DuplicateClip(clip.id().clone()));
            }
            let duration = clip.timeline_range().duration();
            if duration.is_zero() {
                return Err(EditError::InvalidDuration);
            }
            let insert_index = match after {
                None => 0,
                Some(anchor) => {
                    find_clip_index(project, anchor)
                        .map_err(|_| EditError::MissingAnchor(anchor.clone()))?
                        + 1
                }
            };
            let start = if insert_index == 0 {
                0
            } else {
                project.root().clips()[insert_index - 1]
                    .timeline_range()
                    .start()
                    .value()
                    .checked_add(
                        project.root().clips()[insert_index - 1]
                            .timeline_range()
                            .duration()
                            .value(),
                    )
                    .ok_or(EditError::ArithmeticOverflow)?
            };
            for following in &mut project.root_mut().clips_mut()[insert_index..] {
                let range = following.timeline_range();
                let shifted = range
                    .start()
                    .value()
                    .checked_add(duration.value())
                    .ok_or(EditError::ArithmeticOverflow)?;
                following.set_timeline_range(FrameRange::new(
                    FrameIndex::new(shifted),
                    range.duration(),
                ));
            }
            let mut inserted = clip.clone();
            inserted.set_timeline_range(FrameRange::new(FrameIndex::new(start), duration));
            if !project.root_mut().insert_clip(insert_index, inserted) {
                return Err(EditError::ArithmeticOverflow);
            }
            Ok(())
        }
        SemanticEdit::MoveAfter { clip, after } => {
            let clip_index = find_clip_index(project, clip)?;
            if after.as_ref() == Some(clip) {
                return Err(EditError::SelfAnchor(clip.clone()));
            }
            let moved = project.root_mut().remove_clip(clip_index);
            let insert_index = match after {
                None => 0,
                Some(anchor) => {
                    find_clip_index(project, anchor)
                        .map_err(|_| EditError::MissingAnchor(anchor.clone()))?
                        + 1
                }
            };
            if !project.root_mut().insert_clip(insert_index, moved) {
                return Err(EditError::ArithmeticOverflow);
            }
            reflow(project)
        }
        SemanticEdit::AddMarker { id, clip, label } => {
            validate_metadata(project, id, clip, label)?;
            project
                .root_mut()
                .add_marker(Marker::new(id.clone(), clip.clone(), label.clone()));
            Ok(())
        }
        SemanticEdit::AddSubtitle { id, clip, text } => {
            validate_metadata(project, id, clip, text)?;
            project.root_mut().add_subtitle(SubtitleCue::new(
                id.clone(),
                clip.clone(),
                text.clone(),
            ));
            Ok(())
        }
        SemanticEdit::AddTransition(transition) => transition
            .validate(project)
            .map_err(EditError::InvalidTransition),
    }
}

/// A planned render transaction bound to the revision it observed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanToken {
    base_revision: Revision,
    payload: EditPayload,
    plan: RenderPlan,
}

impl PlanToken {
    pub fn base_revision(&self) -> Revision {
        self.base_revision
    }

    pub fn payload(&self) -> &EditPayload {
        &self.payload
    }

    pub fn plan(&self) -> &RenderPlan {
        &self.plan
    }
}

/// Result of validating a planned transaction before commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerificationResult {
    revision: Revision,
    passed: bool,
}

impl VerificationResult {
    pub const fn revision(self) -> Revision {
        self.revision
    }

    pub const fn passed(self) -> bool {
        self.passed
    }
}

/// A plan that passed runtime invariants and is eligible for commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedPlan {
    token: PlanToken,
    result: VerificationResult,
}

impl VerifiedPlan {
    pub fn plan(&self) -> &RenderPlan {
        self.token.plan()
    }

    pub const fn result(&self) -> VerificationResult {
        self.result
    }
}

/// Errors returned when a transaction cannot be planned or committed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransactionError {
    StaleRevision {
        expected: Revision,
        actual: Revision,
    },
    RevisionExhausted,
    EmptyRenderUnit {
        index: usize,
    },
    OverlappingRenderUnits {
        previous: usize,
        current: usize,
    },
    RenderPlanOverflow {
        index: usize,
    },
    InvalidEdit {
        index: usize,
        source: EditError,
    },
}

impl fmt::Display for TransactionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleRevision { expected, actual } => write!(
                formatter,
                "stale revision: expected {}, actual {}",
                expected.value(),
                actual.value()
            ),
            Self::RevisionExhausted => write!(formatter, "project revision is exhausted"),
            Self::EmptyRenderUnit { index } => {
                write!(formatter, "render unit {index} has zero duration")
            }
            Self::OverlappingRenderUnits { previous, current } => {
                write!(formatter, "render units overlap: {previous} and {current}")
            }
            Self::RenderPlanOverflow { index } => {
                write!(formatter, "render unit {index} exceeds the time domain")
            }
            Self::InvalidEdit { index, source } => {
                write!(formatter, "semantic edit {index} is invalid: {source}")
            }
        }
    }
}

impl std::error::Error for TransactionError {}

#[derive(Default)]
pub struct Runtime {
    planner: Planner,
    revision: Revision,
}

impl Runtime {
    pub fn from_store(store: &impl RevisionStore) -> Result<Self, PersistenceError> {
        Ok(Self {
            planner: Planner::default(),
            revision: store.load()?,
        })
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn plan(&self, project: &raccord_ir::Project) -> RenderPlan {
        self.planner.plan(project)
    }

    pub fn plan_at(
        &self,
        project: &raccord_ir::Project,
        base_revision: Revision,
    ) -> Result<PlanToken, TransactionError> {
        self.plan_edit_at(project, base_revision, EditPayload::default())
    }

    pub fn plan_edit_at(
        &self,
        project: &raccord_ir::Project,
        base_revision: Revision,
        payload: EditPayload,
    ) -> Result<PlanToken, TransactionError> {
        if base_revision != self.revision {
            return Err(TransactionError::StaleRevision {
                expected: self.revision,
                actual: base_revision,
            });
        }
        let edited_project = payload.apply(project)?;
        Ok(PlanToken {
            base_revision,
            payload,
            plan: self.plan(&edited_project),
        })
    }

    pub fn verify(&self, token: PlanToken) -> Result<VerifiedPlan, TransactionError> {
        if token.base_revision != self.revision {
            return Err(TransactionError::StaleRevision {
                expected: self.revision,
                actual: token.base_revision,
            });
        }
        for (index, unit) in token.plan.units.iter().enumerate() {
            if unit.frame_count == 0 {
                return Err(TransactionError::EmptyRenderUnit { index });
            }
            if unit.start_frame.checked_add(unit.frame_count).is_none() {
                return Err(TransactionError::RenderPlanOverflow { index });
            }
            if let Some((previous_index, previous)) = token
                .plan
                .units
                .get(..index)
                .and_then(|units| units.iter().enumerate().next_back())
            {
                let previous_end = previous
                    .start_frame
                    .checked_add(previous.frame_count)
                    .ok_or(TransactionError::RenderPlanOverflow {
                        index: previous_index,
                    })?;
                if previous_end > unit.start_frame {
                    return Err(TransactionError::OverlappingRenderUnits {
                        previous: previous_index,
                        current: index,
                    });
                }
            }
        }
        Ok(VerifiedPlan {
            token,
            result: VerificationResult {
                revision: self.revision,
                passed: true,
            },
        })
    }

    fn next_commit_revision(
        &self,
        transaction: &VerifiedPlan,
    ) -> Result<Revision, TransactionError> {
        if transaction.result.revision != self.revision
            || transaction.token.base_revision != self.revision
        {
            return Err(TransactionError::StaleRevision {
                expected: self.revision,
                actual: transaction.token.base_revision,
            });
        }
        self.revision.next()
    }

    pub fn commit(&mut self, transaction: VerifiedPlan) -> Result<Revision, TransactionError> {
        let revision = self.next_commit_revision(&transaction)?;
        self.revision = revision;
        Ok(revision)
    }

    pub fn commit_with_store(
        &mut self,
        transaction: VerifiedPlan,
        store: &impl RevisionStore,
    ) -> Result<Revision, CommitError> {
        let revision = self
            .next_commit_revision(&transaction)
            .map_err(CommitError::Transaction)?;
        store.save(revision).map_err(CommitError::Persistence)?;
        self.revision = revision;
        Ok(revision)
    }

    pub fn action_result(&self, result: ActionResult) -> ActionResult {
        result
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use raccord_ir::{Clip, ClipId, Composition, Project, ProjectId, SourceRef};
    use raccord_time::{FrameCount, FrameIndex, FrameRange, FrameRate};

    use super::{
        EditError, EditPayload, FileRevisionStore, PlanToken, Revision, RevisionStore, Runtime,
        SemanticEdit, TransactionError,
    };
    use raccord_planner::{RenderPlan, RenderUnit};
    use raccord_timeline::{Transition, TransitionError, TransitionKind};

    static STORE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn revision_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "raccord-runtime-revision-{}-{}",
            std::process::id(),
            STORE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn project() -> Project {
        Project::new(
            ProjectId::new("fixture").expect("non-empty project id"),
            Composition::new(FrameRate::new(24, 1).expect("valid frame rate")),
        )
    }

    fn project_with_clips() -> Project {
        let mut composition = Composition::new(FrameRate::new(24, 1).expect("valid frame rate"));
        for (id, start) in [("a", 0), ("b", 48)] {
            composition.add_clip(Clip::new(
                ClipId::new(id).expect("non-empty clip id"),
                SourceRef::new(format!("asset://{id}")).expect("non-empty source"),
                FrameRange::new(FrameIndex::new(start), FrameCount::new(48)),
            ));
        }
        Project::new(
            ProjectId::new("fixture").expect("non-empty project id"),
            composition,
        )
    }

    #[test]
    fn plan_and_commit_advance_the_revision() {
        let mut runtime = Runtime::default();
        let token = runtime
            .plan_at(&project(), Revision::ZERO)
            .expect("current revision can be planned");

        assert_eq!(token.base_revision(), Revision::ZERO);
        assert!(token.plan().units.is_empty());
        let verified = runtime.verify(token).expect("plan invariants pass");
        assert!(verified.result().passed());
        assert_eq!(runtime.commit(verified), Ok(Revision::new(1)));
        assert_eq!(runtime.revision(), Revision::new(1));
    }

    #[test]
    fn revision_store_survives_runtime_reconstruction() {
        let path = revision_path();
        let store = FileRevisionStore::new(&path);
        let mut runtime = Runtime::from_store(&store).expect("missing revision starts at zero");
        let token = runtime
            .plan_at(&project(), Revision::ZERO)
            .expect("current revision can be planned");
        let verified = runtime.verify(token).expect("plan invariants pass");

        assert_eq!(
            runtime
                .commit_with_store(verified, &store)
                .expect("revision can be persisted"),
            Revision::new(1)
        );
        assert_eq!(
            store.load().expect("revision can be loaded"),
            Revision::new(1)
        );
        assert_eq!(
            Runtime::from_store(&store)
                .expect("persisted revision can be loaded")
                .revision(),
            Revision::new(1)
        );

        fs::remove_file(path).expect("revision file can be removed");
    }

    #[test]
    fn invalid_persisted_revision_is_structured() {
        let path = revision_path();
        fs::write(&path, b"not-a-revision").expect("revision file can be written");
        let store = FileRevisionStore::new(&path);

        assert!(matches!(
            store.load(),
            Err(super::PersistenceError::InvalidRevision(value)) if value == "not-a-revision"
        ));
        fs::remove_file(path).expect("revision file can be removed");
    }

    #[test]
    fn semantic_payload_is_validated_and_carried_by_the_token() {
        let runtime = Runtime::default();
        let transition = Transition::new(
            "crossfade-a-b",
            ClipId::new("a").expect("valid id"),
            ClipId::new("b").expect("valid id"),
            TransitionKind::Crossfade,
            FrameCount::new(24),
        )
        .expect("transition is well formed");
        let payload = EditPayload::new(vec![SemanticEdit::AddTransition(transition)]);
        let token = runtime
            .plan_edit_at(&project_with_clips(), Revision::ZERO, payload)
            .expect("semantic payload is valid");

        assert_eq!(token.payload().edits().len(), 1);
        assert!(!token.payload().is_empty());
    }

    #[test]
    fn metadata_edits_are_validated_and_carried_by_the_token() {
        let runtime = Runtime::default();
        let token = runtime
            .plan_edit_at(
                &project_with_clips(),
                Revision::ZERO,
                EditPayload::new(vec![
                    SemanticEdit::AddMarker {
                        id: "marker-1".into(),
                        clip: ClipId::new("a").expect("valid id"),
                        label: "cut point".into(),
                    },
                    SemanticEdit::AddSubtitle {
                        id: "subtitle-1".into(),
                        clip: ClipId::new("b").expect("valid id"),
                        text: "hello".into(),
                    },
                ]),
            )
            .expect("metadata edits are valid");

        assert_eq!(token.payload().edits().len(), 2);
    }

    #[test]
    fn duplicate_metadata_is_rejected_inside_one_payload() {
        let runtime = Runtime::default();
        assert!(matches!(
            runtime.plan_edit_at(
                &project_with_clips(),
                Revision::ZERO,
                EditPayload::new(vec![
                    SemanticEdit::AddMarker {
                        id: "same-id".into(),
                        clip: ClipId::new("a").expect("valid id"),
                        label: "first".into(),
                    },
                    SemanticEdit::AddSubtitle {
                        id: "same-id".into(),
                        clip: ClipId::new("b").expect("valid id"),
                        text: "second".into(),
                    },
                ]),
            ),
            Err(TransactionError::InvalidEdit {
                index: 1,
                source: EditError::DuplicateMetadata(id),
            }) if id == "same-id"
        ));
    }

    #[test]
    fn semantic_edits_update_the_planned_render_units() {
        let runtime = Runtime::default();
        let payload = EditPayload::new(vec![
            SemanticEdit::RippleDelete {
                clip: ClipId::new("a").expect("valid id"),
            },
            SemanticEdit::Trim {
                clip: ClipId::new("b").expect("valid id"),
                duration: FrameCount::new(24),
            },
        ]);
        let token = runtime
            .plan_edit_at(&project_with_clips(), Revision::ZERO, payload)
            .expect("semantic edits are valid");

        assert_eq!(
            token.plan().units,
            vec![RenderUnit {
                start_frame: 0,
                frame_count: 24,
            }]
        );
    }

    #[test]
    fn insert_and_move_edits_reflow_the_planned_units() {
        let runtime = Runtime::default();
        let inserted = Clip::new(
            ClipId::new("c").expect("valid id"),
            SourceRef::new("asset://c").expect("valid source"),
            FrameRange::new(FrameIndex::ZERO, FrameCount::new(24)),
        );
        let inserted_token = runtime
            .plan_edit_at(
                &project_with_clips(),
                Revision::ZERO,
                EditPayload::new(vec![SemanticEdit::InsertAfter {
                    clip: inserted,
                    after: Some(ClipId::new("a").expect("valid id")),
                }]),
            )
            .expect("insert is valid");
        assert_eq!(
            inserted_token.plan().units,
            vec![
                RenderUnit {
                    start_frame: 0,
                    frame_count: 48,
                },
                RenderUnit {
                    start_frame: 48,
                    frame_count: 24,
                },
                RenderUnit {
                    start_frame: 72,
                    frame_count: 48,
                },
            ]
        );

        let moved_token = runtime
            .plan_edit_at(
                &project_with_clips(),
                Revision::ZERO,
                EditPayload::new(vec![SemanticEdit::MoveAfter {
                    clip: ClipId::new("b").expect("valid id"),
                    after: None,
                }]),
            )
            .expect("move is valid");
        assert_eq!(
            moved_token.plan().units,
            vec![
                RenderUnit {
                    start_frame: 0,
                    frame_count: 48,
                },
                RenderUnit {
                    start_frame: 48,
                    frame_count: 48,
                },
            ]
        );
    }

    #[test]
    fn invalid_semantic_payload_is_rejected_before_planning() {
        let runtime = Runtime::default();
        let transition = Transition::new(
            "crossfade-a-c",
            ClipId::new("a").expect("valid id"),
            ClipId::new("c").expect("valid id"),
            TransitionKind::Crossfade,
            FrameCount::new(24),
        )
        .expect("transition is well formed");

        assert!(matches!(
            runtime.plan_edit_at(
                &project_with_clips(),
                Revision::ZERO,
                EditPayload::new(vec![SemanticEdit::AddTransition(transition)]),
            ),
            Err(TransactionError::InvalidEdit {
                index: 0,
                source: EditError::InvalidTransition(TransitionError::MissingClip(_)),
            })
        ));
    }

    #[test]
    fn verification_rejects_zero_duration_units() {
        let runtime = Runtime::default();
        let token = PlanToken {
            base_revision: Revision::ZERO,
            payload: super::EditPayload::default(),
            plan: RenderPlan {
                units: vec![RenderUnit {
                    start_frame: 0,
                    frame_count: 0,
                }],
            },
        };

        assert_eq!(
            runtime.verify(token),
            Err(TransactionError::EmptyRenderUnit { index: 0 })
        );
    }

    #[test]
    fn stale_revision_is_rejected_before_planning() {
        let runtime = Runtime::default();

        assert_eq!(
            runtime.plan_at(&project(), Revision::new(7)),
            Err(TransactionError::StaleRevision {
                expected: Revision::ZERO,
                actual: Revision::new(7),
            })
        );
    }

    #[test]
    fn committed_tokens_cannot_be_replayed() {
        let mut runtime = Runtime::default();
        let token = runtime
            .plan_at(&project(), Revision::ZERO)
            .expect("current revision can be planned");
        let replay = token.clone();
        let verified = runtime.verify(token).expect("plan invariants pass");
        let replay_verified = runtime.verify(replay).expect("plan invariants pass");

        assert_eq!(runtime.commit(verified), Ok(Revision::new(1)));
        assert_eq!(
            runtime.commit(replay_verified),
            Err(TransactionError::StaleRevision {
                expected: Revision::new(1),
                actual: Revision::ZERO,
            })
        );
    }
}
