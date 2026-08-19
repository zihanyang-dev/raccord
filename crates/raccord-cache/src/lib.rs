#![forbid(unsafe_code)]

#[path = "error.rs"]
mod error;
#[path = "index.rs"]
mod index;
#[path = "key.rs"]
mod key;
#[path = "store.rs"]
mod store;

pub use error::StoreError;
pub use index::{ArtifactMetadata, CacheEntry, CacheIndex};
pub use key::CacheKey;
pub use store::{ArtifactStore, CacheLock, LockOptions};

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    use super::{ArtifactStore, CacheEntry, CacheIndex, CacheKey, LockOptions, StoreError};
    use raccord_media::ArtifactRef;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "raccord-cache-test-{}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn cache_keys_are_deterministic_and_sensitive_to_parts() {
        let first =
            CacheKey::from_parts("media", &["clip-b", "gain=-3000"]).expect("parts are non-empty");
        let same =
            CacheKey::from_parts("media", &["clip-b", "gain=-3000"]).expect("parts are non-empty");
        let changed =
            CacheKey::from_parts("media", &["clip-b", "gain=-6000"]).expect("parts are non-empty");

        assert_eq!(first, same);
        assert_ne!(first, changed);
        assert!(first.as_str().starts_with("media-"));
    }

    #[test]
    fn cache_index_replaces_and_reads_entries_by_key() {
        let key = CacheKey::from_parts("media", &["clip-a"]).expect("parts are non-empty");
        let artifact = ArtifactRef::new("sha256:artifact-a").expect("digest is non-empty");
        let mut index = CacheIndex::default();
        let entry = CacheEntry {
            key: key.clone(),
            artifact: artifact.clone(),
            metadata: super::ArtifactMetadata { byte_len: 12 },
        };

        assert!(index.insert(entry).is_none());
        assert_eq!(
            index.get(&key).map(|entry| entry.artifact.clone()),
            Some(artifact)
        );
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn filesystem_store_publishes_and_reuses_artifacts() {
        let root = test_root();
        let source = root.join("source.mp4");
        let destination = root.join("restored.mp4");
        fs::create_dir_all(&root).expect("test root can be created");
        fs::write(&source, b"reference artifact").expect("source can be written");

        let key = CacheKey::from_parts("media", &["fixture"]).expect("parts are non-empty");
        let artifact = ArtifactRef::new("sha256:fixture").expect("digest is non-empty");
        let store = ArtifactStore::new(&root).expect("store can be created");
        let written = store
            .put_file(&key, artifact.clone(), &source)
            .expect("artifact can be published");
        let restored = store
            .copy_to(&key, &destination)
            .expect("artifact can be copied")
            .expect("artifact exists");

        assert_eq!(written.artifact, artifact);
        assert_eq!(written.metadata.byte_len, 18);
        assert_eq!(restored.metadata, written.metadata);
        assert_eq!(
            fs::read(destination).expect("destination can be read"),
            b"reference artifact"
        );
        assert_eq!(
            store.get(&key).expect("artifact can be loaded"),
            Some(written)
        );

        fs::remove_dir_all(root).expect("test root can be removed");
    }

    #[test]
    fn stale_locks_are_reclaimed_without_old_guard_deleting_new_owner() {
        let root = test_root();
        let store = ArtifactStore::new(&root).expect("store can be created");
        let key = CacheKey::from_parts("media", &["fixture"]).expect("parts are non-empty");
        let old = store
            .acquire_key_lock(&key)
            .expect("first lock can be acquired");
        let current = store
            .acquire_key_lock_with(&key, LockOptions::new(Duration::ZERO))
            .expect("zero-age lock can be reclaimed");

        drop(old);
        assert!(matches!(
            store.acquire_key_lock(&key),
            Err(StoreError::LockHeld(ref locked)) if locked == &key
        ));
        drop(current);
        fs::remove_dir_all(root).expect("test root can be removed");
    }

    #[test]
    fn invalid_destination_returns_a_structured_error() {
        let root = test_root();
        let source = root.join("source.mp4");
        fs::create_dir_all(&root).expect("test root can be created");
        fs::write(&source, b"artifact").expect("source can be written");
        let store = ArtifactStore::new(&root).expect("store can be created");
        let key = CacheKey::from_parts("media", &["fixture"]).expect("parts are non-empty");
        let artifact = ArtifactRef::new("sha256:fixture").expect("digest is non-empty");

        assert!(store.put_file(&key, artifact, &source).is_ok());
        assert!(matches!(
            store.copy_to(&key, Path::new("/")),
            Err(StoreError::InvalidDestination(path)) if path == Path::new("/")
        ));
        fs::remove_dir_all(root).expect("test root can be removed");
    }

    #[test]
    fn key_locks_are_exclusive_and_release_on_drop() {
        let root = test_root();
        let store = ArtifactStore::new(&root).expect("store can be created");
        let key = CacheKey::from_parts("media", &["fixture"]).expect("parts are non-empty");
        let first = store
            .acquire_key_lock(&key)
            .expect("first lock can be acquired");

        assert!(matches!(
            store.acquire_key_lock(&key),
            Err(StoreError::LockHeld(ref locked)) if locked == &key
        ));
        drop(first);
        let second = store
            .acquire_key_lock(&key)
            .expect("lock is released when its guard drops");
        drop(second);

        fs::remove_dir_all(root).expect("test root can be removed");
    }
}
