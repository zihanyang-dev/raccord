use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use raccord_media::ArtifactRef;
use serde::{Deserialize, Serialize};

use crate::{ArtifactMetadata, CacheEntry, CacheKey, StoreError};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize, Serialize)]
struct Manifest {
    artifact_digest: String,
    byte_len: u64,
}

#[derive(Serialize)]
struct LockOwner {
    process_id: u32,
    token: u64,
    created_at_unix_secs: u64,
}

/// Policy for reclaiming a lock left by a crashed process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockOptions {
    /// Age after which an abandoned lock may be reclaimed.
    pub stale_after: Duration,
}

impl LockOptions {
    pub const fn new(stale_after: Duration) -> Self {
        Self { stale_after }
    }
}

impl Default for LockOptions {
    fn default() -> Self {
        Self::new(Duration::from_secs(30 * 60))
    }
}

/// RAII guard for a cache-key lock.
pub struct CacheLock {
    path: PathBuf,
    owner_path: PathBuf,
    owner_contents: Vec<u8>,
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        let owned = fs::read(&self.owner_path)
            .map(|contents| contents == self.owner_contents)
            .unwrap_or(false);
        if owned {
            let _ = fs::remove_file(&self.owner_path);
            let _ = fs::remove_dir(&self.path);
        }
    }
}

/// Filesystem-backed artifact store with atomic publication of artifacts and manifests.
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    /// Open a store rooted at `root`, creating it when necessary.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Read and validate an artifact entry, returning `None` on a cache miss.
    pub fn get(&self, key: &CacheKey) -> Result<Option<CacheEntry>, StoreError> {
        let artifact_path = self.artifact_path(key);
        let manifest_path = self.manifest_path(key);
        let artifact_exists = artifact_path.is_file();
        let manifest_exists = manifest_path.is_file();
        if !artifact_exists && !manifest_exists {
            return Ok(None);
        }
        if !artifact_exists || !manifest_exists {
            return Err(StoreError::IncompleteArtifact(key.clone()));
        }

        let manifest: Manifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
        let artifact = ArtifactRef::new(manifest.artifact_digest)
            .ok_or_else(|| StoreError::CorruptArtifact(key.clone()))?;
        let actual_len = fs::metadata(artifact_path)?.len();
        if actual_len != manifest.byte_len {
            return Err(StoreError::CorruptArtifact(key.clone()));
        }
        Ok(Some(CacheEntry {
            key: key.clone(),
            artifact,
            metadata: ArtifactMetadata {
                byte_len: manifest.byte_len,
            },
        }))
    }

    /// Acquire an exclusive lock for work associated with one semantic key.
    pub fn acquire_key_lock(&self, key: &CacheKey) -> Result<CacheLock, StoreError> {
        self.acquire_key_lock_with(key, LockOptions::default())
    }

    /// Acquire a key lock and reclaim it when its directory is older than the policy.
    pub fn acquire_key_lock_with(
        &self,
        key: &CacheKey,
        options: LockOptions,
    ) -> Result<CacheLock, StoreError> {
        let path = self.lock_path(key);
        loop {
            match fs::create_dir(&path) {
                Ok(()) => return self.finish_lock_acquisition(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if self.lock_is_stale(&path, options.stale_after)? {
                        match fs::remove_dir_all(&path) {
                            Ok(()) => continue,
                            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                            Err(error) => return Err(StoreError::Io(error)),
                        }
                    }
                    return Err(StoreError::LockHeld(key.clone()));
                }
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
    }

    /// Atomically publish a source file and its manifest under `key`.
    pub fn put_file(
        &self,
        key: &CacheKey,
        artifact: ArtifactRef,
        source: &Path,
    ) -> Result<CacheEntry, StoreError> {
        let _lock = self.acquire_key_lock(key)?;
        let byte_len = fs::metadata(source)?.len();
        let artifact_path = self.artifact_path(key);
        atomic_copy(source, &artifact_path)?;

        let manifest = Manifest {
            artifact_digest: artifact.digest().to_owned(),
            byte_len,
        };
        let manifest_bytes = serde_json::to_vec(&manifest)?;
        atomic_write(&self.manifest_path(key), &manifest_bytes)?;

        Ok(CacheEntry {
            key: key.clone(),
            artifact,
            metadata: ArtifactMetadata { byte_len },
        })
    }

    /// Copy a validated cached artifact to `destination`.
    pub fn copy_to(
        &self,
        key: &CacheKey,
        destination: &Path,
    ) -> Result<Option<CacheEntry>, StoreError> {
        let Some(entry) = self.get(key)? else {
            return Ok(None);
        };
        atomic_copy(&self.artifact_path(key), destination)?;
        Ok(Some(entry))
    }

    /// Remove an artifact and its manifest, returning whether either existed.
    pub fn remove(&self, key: &CacheKey) -> Result<bool, StoreError> {
        let artifact_path = self.artifact_path(key);
        let manifest_path = self.manifest_path(key);
        let existed = artifact_path.exists() || manifest_path.exists();
        remove_if_present(&artifact_path)?;
        remove_if_present(&manifest_path)?;
        Ok(existed)
    }

    fn artifact_path(&self, key: &CacheKey) -> PathBuf {
        self.root.join(format!("{}.artifact", key.as_str()))
    }

    fn manifest_path(&self, key: &CacheKey) -> PathBuf {
        self.root.join(format!("{}.manifest.json", key.as_str()))
    }

    fn lock_path(&self, key: &CacheKey) -> PathBuf {
        self.root.join(format!("{}.lock", key.as_str()))
    }

    fn finish_lock_acquisition(&self, path: PathBuf) -> Result<CacheLock, StoreError> {
        let token = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let owner = LockOwner {
            process_id: std::process::id(),
            token,
            created_at_unix_secs: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let owner_contents = serde_json::to_vec(&owner).map_err(StoreError::LockMetadata)?;
        let owner_path = path.join("owner.json");
        if let Err(error) = fs::write(&owner_path, &owner_contents) {
            let _ = fs::remove_dir_all(&path);
            return Err(StoreError::Io(error));
        }
        Ok(CacheLock {
            path,
            owner_path,
            owner_contents,
        })
    }

    fn lock_is_stale(&self, path: &Path, stale_after: Duration) -> Result<bool, StoreError> {
        let modified = match fs::metadata(path) {
            Ok(metadata) => metadata.modified()?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(StoreError::Io(error)),
        };
        Ok(modified
            .elapsed()
            .map(|age| age >= stale_after)
            .unwrap_or(false))
    }
}

fn atomic_copy(source: &Path, destination: &Path) -> Result<(), StoreError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = temporary_path(destination)?;
    fs::copy(source, &temporary)?;
    fs::rename(temporary, destination)?;
    Ok(())
}

fn atomic_write(destination: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = temporary_path(destination)?;
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, destination)?;
    Ok(())
}

fn temporary_path(destination: &Path) -> Result<PathBuf, StoreError> {
    let Some(file_name) = destination.file_name() else {
        return Err(StoreError::InvalidDestination(destination.to_path_buf()));
    };
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = file_name.to_string_lossy();
    Ok(destination.with_file_name(format!(".{file_name}.{}.tmp", counter)))
}

fn remove_if_present(path: &Path) -> Result<(), StoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StoreError::Io(error)),
    }
}
