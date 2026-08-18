#![forbid(unsafe_code)]

use raccord_media::ArtifactRef;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionKind {
    Prepare,
    Inspect,
    Execute,
    Cancel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionResult {
    pub kind: ActionKind,
    pub outputs: Vec<ArtifactRef>,
}
