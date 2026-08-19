use std::{fmt, io, path::PathBuf};

use crate::CacheKey;

/// Errors produced while reading or publishing filesystem artifacts.
#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Manifest(serde_json::Error),
    LockMetadata(serde_json::Error),
    IncompleteArtifact(CacheKey),
    CorruptArtifact(CacheKey),
    InvalidDestination(PathBuf),
    LockHeld(CacheKey),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "cache I/O error: {error}"),
            Self::Manifest(error) => write!(formatter, "cache manifest error: {error}"),
            Self::LockMetadata(error) => write!(formatter, "cache lock metadata error: {error}"),
            Self::IncompleteArtifact(key) => {
                write!(formatter, "cache artifact is incomplete: {}", key.as_str())
            }
            Self::CorruptArtifact(key) => {
                write!(formatter, "cache artifact is corrupt: {}", key.as_str())
            }
            Self::InvalidDestination(path) => {
                write!(
                    formatter,
                    "cache destination has no file name: {}",
                    path.display()
                )
            }
            Self::LockHeld(key) => {
                write!(formatter, "cache key is locked: {}", key.as_str())
            }
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Manifest(error)
    }
}
