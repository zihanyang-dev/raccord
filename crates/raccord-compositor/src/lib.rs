#![forbid(unsafe_code)]

use raccord_ir::SourceRef;
use raccord_media::MediaKind;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layer {
    source: SourceRef,
    kind: MediaKind,
}

impl Layer {
    pub const fn new(source: SourceRef, kind: MediaKind) -> Self {
        Self { source, kind }
    }

    pub fn source(&self) -> &SourceRef {
        &self.source
    }

    pub const fn kind(&self) -> MediaKind {
        self.kind
    }
}
