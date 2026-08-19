use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufRead},
    path::Path,
    process,
};

use raccord_product_path_experiment::{
    EditError, Preview, SemanticEdit, Timeline, Transaction, demo_timeline, plan_edit, verify,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct Request {
    tool: String,
    #[serde(default)]
    args: Value,
}

#[derive(Debug, Deserialize)]
struct PlanArgs {
    base_revision: u64,
    edits: Vec<SemanticEdit>,
}

#[derive(Debug, Deserialize)]
struct CommitArgs {
    plan_token: String,
}

#[derive(Debug, Deserialize, Default)]
struct InspectArgs {
    ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct FindArgs {
    query: String,
    #[serde(default = "default_find_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct CacheKeyArgs {
    media_hashes: BTreeMap<String, String>,
}

fn default_find_limit() -> usize {
    5
}

#[derive(Debug, Serialize)]
struct Response<T> {
    ok: bool,
    result: Option<T>,
    error: Option<ErrorBody>,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: String,
    detail: String,
}

#[derive(Debug, Serialize)]
struct InspectResult {
    revision: u64,
    clips: Vec<raccord_product_path_experiment::Clip>,
    placements: Vec<raccord_product_path_experiment::Placement>,
    markers: Vec<raccord_product_path_experiment::Marker>,
    subtitles: Vec<raccord_product_path_experiment::SubtitleCue>,
    transitions: Vec<raccord_product_path_experiment::Transition>,
}

#[derive(Debug, Serialize)]
struct FindResult {
    matches: Vec<raccord_product_path_experiment::Clip>,
}

#[derive(Debug, Serialize)]
struct PlanResult {
    plan_token: String,
    version: u64,
    changed_clip_ids: Vec<String>,
    placements: Vec<raccord_product_path_experiment::Placement>,
    markers: Vec<raccord_product_path_experiment::Marker>,
    subtitles: Vec<raccord_product_path_experiment::SubtitleCue>,
    transitions: Vec<raccord_product_path_experiment::Transition>,
}

#[derive(Debug, Serialize)]
struct CommitResult {
    version: u64,
    placements: Vec<raccord_product_path_experiment::Placement>,
    markers: Vec<raccord_product_path_experiment::Marker>,
    subtitles: Vec<raccord_product_path_experiment::SubtitleCue>,
    transitions: Vec<raccord_product_path_experiment::Transition>,
}

#[derive(Debug, Serialize)]
struct VerifyResult {
    pass: bool,
    version: u64,
}

#[derive(Debug, Serialize)]
struct CacheKeyResult {
    media_render_key: String,
    subtitle_overlay_key: String,
    metadata_key: String,
    transition_render_key: String,
    clip_media_render_keys: BTreeMap<String, String>,
}

struct App {
    timeline: Timeline,
    plans: BTreeMap<String, Preview>,
    next_plan: u64,
}

impl App {
    fn from_timeline(timeline: Timeline) -> Self {
        Self {
            timeline,
            plans: BTreeMap::new(),
            next_plan: 1,
        }
    }

    fn handle(&mut self, request: Request) -> Response<Value> {
        match request.tool.as_str() {
            "find" => self.find(request.args),
            "inspect" => self.inspect(request.args),
            "plan_edit" => self.plan_edit(request.args),
            "commit_edit" => self.commit_edit(request.args),
            "verify" => self.verify(),
            "cache_keys" => self.cache_keys(request.args),
            tool => failure("UNKNOWN_TOOL", format!("unsupported tool: {tool}")),
        }
    }

    fn find(&self, args: Value) -> Response<Value> {
        let args = match serde_json::from_value::<FindArgs>(args) {
            Ok(args) => args,
            Err(error) => return failure("INVALID_ARGUMENTS", error.to_string()),
        };
        let query = args.query.to_lowercase();
        let matches = self
            .timeline
            .clips
            .iter()
            .filter(|clip| {
                clip.id.to_lowercase().contains(&query)
                    || clip.source.to_lowercase().contains(&query)
            })
            .take(args.limit)
            .cloned()
            .collect();

        success(FindResult { matches })
    }

    fn inspect(&self, args: Value) -> Response<Value> {
        let args = if args.is_null() {
            InspectArgs::default()
        } else {
            match serde_json::from_value::<InspectArgs>(args) {
                Ok(args) => args,
                Err(error) => return failure("INVALID_ARGUMENTS", error.to_string()),
            }
        };
        let selected_ids = args.ids;
        let clips = self
            .timeline
            .clips
            .iter()
            .filter(|clip| {
                selected_ids
                    .as_ref()
                    .is_none_or(|ids| ids.iter().any(|id| id == &clip.id))
            })
            .cloned()
            .collect();
        let placements = self
            .timeline
            .placements()
            .into_iter()
            .filter(|placement| {
                selected_ids
                    .as_ref()
                    .is_none_or(|ids| ids.iter().any(|id| id == &placement.id))
            })
            .collect();
        let markers = self
            .timeline
            .markers
            .iter()
            .filter(|marker| {
                selected_ids
                    .as_ref()
                    .is_none_or(|ids| ids.iter().any(|id| id == &marker.clip_id))
            })
            .cloned()
            .collect();
        let subtitles = self
            .timeline
            .subtitles
            .iter()
            .filter(|subtitle| {
                selected_ids
                    .as_ref()
                    .is_none_or(|ids| ids.iter().any(|id| id == &subtitle.clip_id))
            })
            .cloned()
            .collect();

        success(InspectResult {
            revision: self.timeline.revision,
            clips,
            placements,
            markers,
            subtitles,
            transitions: self.timeline.transitions.clone(),
        })
    }

    fn plan_edit(&mut self, args: Value) -> Response<Value> {
        let args = match serde_json::from_value::<PlanArgs>(args) {
            Ok(args) => args,
            Err(error) => return failure("INVALID_ARGUMENTS", error.to_string()),
        };
        let transaction = Transaction {
            base_revision: args.base_revision,
            edits: args.edits,
        };
        let preview = match plan_edit(&self.timeline, &transaction) {
            Ok(preview) => preview,
            Err(error) => return edit_failure(error),
        };

        let token = format!("plan-{}", self.next_plan);
        self.next_plan += 1;
        let result = PlanResult {
            plan_token: token.clone(),
            version: preview.timeline.revision,
            changed_clip_ids: preview.changed_clip_ids.clone(),
            placements: preview.timeline.placements(),
            markers: preview.timeline.markers.clone(),
            subtitles: preview.timeline.subtitles.clone(),
            transitions: preview.timeline.transitions.clone(),
        };
        self.plans.insert(token, preview);
        success(result)
    }

    fn commit_edit(&mut self, args: Value) -> Response<Value> {
        let args = match serde_json::from_value::<CommitArgs>(args) {
            Ok(args) => args,
            Err(error) => return failure("INVALID_ARGUMENTS", error.to_string()),
        };
        let Some(preview) = self.plans.remove(&args.plan_token) else {
            return failure("UNKNOWN_PLAN", args.plan_token);
        };
        self.timeline = preview.timeline;

        success(CommitResult {
            version: self.timeline.revision,
            placements: self.timeline.placements(),
            markers: self.timeline.markers.clone(),
            subtitles: self.timeline.subtitles.clone(),
            transitions: self.timeline.transitions.clone(),
        })
    }

    fn verify(&self) -> Response<Value> {
        success(VerifyResult {
            pass: verify(&self.timeline),
            version: self.timeline.revision,
        })
    }

    fn cache_keys(&self, args: Value) -> Response<Value> {
        let args = match serde_json::from_value::<CacheKeyArgs>(args) {
            Ok(args) => args,
            Err(error) => return failure("INVALID_ARGUMENTS", error.to_string()),
        };
        match raccord_product_path_experiment::cache::cache_keys(&self.timeline, &args.media_hashes)
        {
            Ok(keys) => success(CacheKeyResult {
                media_render_key: keys.media_render_key,
                subtitle_overlay_key: keys.subtitle_overlay_key,
                metadata_key: keys.metadata_key,
                transition_render_key: keys.transition_render_key,
                clip_media_render_keys: keys.clip_media_render_keys,
            }),
            Err(error) => failure("CACHE_KEY_ERROR", format!("{error:?}")),
        }
    }
}

fn success<T: Serialize>(result: T) -> Response<Value> {
    match serde_json::to_value(result) {
        Ok(result) => Response {
            ok: true,
            result: Some(result),
            error: None,
        },
        Err(error) => failure("SERIALIZATION_ERROR", error.to_string()),
    }
}

fn failure(code: &str, detail: impl Into<String>) -> Response<Value> {
    Response {
        ok: false,
        result: None,
        error: Some(ErrorBody {
            code: code.into(),
            detail: detail.into(),
        }),
    }
}

fn edit_failure(error: EditError) -> Response<Value> {
    let code = match &error {
        EditError::StaleRevision { .. } => "STALE_REVISION",
        EditError::MissingClip(_) => "MISSING_CLIP",
        EditError::DuplicateClip(_) => "DUPLICATE_CLIP",
        EditError::MissingAnchor(_) => "MISSING_ANCHOR",
        EditError::DuplicateMetadata(_) => "DUPLICATE_METADATA",
        EditError::InvalidDuration => "INVALID_DURATION",
        EditError::EmptyMetadata => "EMPTY_METADATA",
        EditError::DuplicateTransition(_) => "DUPLICATE_TRANSITION",
        EditError::InvalidTransition => "INVALID_TRANSITION",
    };
    failure(code, format!("{error:?}"))
}

fn initial_timeline() -> Result<Timeline, String> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        None => Ok(demo_timeline()),
        Some("--timeline") => {
            let path = arguments
                .next()
                .ok_or_else(|| "--timeline requires a JSON file path".to_owned())?;
            if arguments.next().is_some() {
                return Err("unexpected arguments after --timeline".to_owned());
            }
            let path = Path::new(&path);
            let contents = fs::read_to_string(path)
                .map_err(|error| format!("cannot read timeline fixture {path:?}: {error}"))?;
            serde_json::from_str(&contents)
                .map_err(|error| format!("cannot parse timeline fixture {path:?}: {error}"))
        }
        Some(argument) => Err(format!("unknown argument: {argument}")),
    }
}

fn main() {
    let timeline = match initial_timeline() {
        Ok(timeline) => timeline,
        Err(error) => {
            eprintln!("timeline configuration error: {error}");
            process::exit(2);
        }
    };
    let stdin = io::stdin();
    let mut app = App::from_timeline(timeline);

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) if !line.trim().is_empty() => line,
            Ok(_) => continue,
            Err(error) => {
                eprintln!("stdin error: {error}");
                process::exit(1);
            }
        };
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => app.handle(request),
            Err(error) => failure("INVALID_JSON", error.to_string()),
        };
        let encoded = match serde_json::to_string(&response) {
            Ok(encoded) => encoded,
            Err(error) => {
                eprintln!("response serialization error: {error}");
                process::exit(1);
            }
        };
        println!("{encoded}");
    }
}
