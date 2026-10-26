//! Recoverable Markdown + CRDT checkpoint publication under an interprocess lock.
//! A synced journal precedes BOTH renames. Recovery completes an interrupted save
//! only when Markdown and metadata match a recorded predecessor or successor.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::document::collaboration::{Collaboration, MAX_STATE_BYTES, ReplicaState};
use crate::io::storage::{FileError, MAX_FILE_BYTES, atomic_write};

const MAGIC: &[u8; 8] = b"STILCR01";
const JOURNAL: &[u8; 8] = b"STILJR01";
const HEADER: usize = 64;

pub fn version(state: &ReplicaState) -> String {
    blake3::hash(&state.update).to_hex().to_string()
}

pub fn encode(state: &ReplicaState) -> Result<Vec<u8>, FileError> {
    if state.update.len() > MAX_STATE_BYTES {
        return Err(FileError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(HEADER + state.update.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(state.document.as_bytes());
    let mut checksum = blake3::Hasher::new();
    checksum
        .update(MAGIC)
        .update(state.document.as_bytes())
        .update(&(state.update.len() as u64).to_le_bytes())
        .update(&state.update);
    bytes.extend_from_slice(checksum.finalize().as_bytes());
    bytes.extend_from_slice(&(state.update.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&state.update);
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Result<ReplicaState, FileError> {
    if bytes.len() < HEADER || &bytes[..8] != MAGIC {
        return Err(invalid("Unsupported collaboration file"));
    }
    let length = u64::from_le_bytes(bytes[56..64].try_into().expect("checked header"));
    if length > MAX_STATE_BYTES as u64 {
        return Err(FileError::TooLarge);
    }
    let mut checksum = blake3::Hasher::new();
    checksum
        .update(&bytes[..24])
        .update(&bytes[56..64])
        .update(&bytes[HEADER..]);
    if bytes.len() - HEADER != length as usize || checksum.finalize().as_bytes() != &bytes[24..56] {
        return Err(invalid("Collaboration checksum or length mismatch"));
    }
    Ok(ReplicaState {
        document: uuid::Uuid::from_slice(&bytes[8..24])
            .map_err(|error| invalid(&error.to_string()))?,
        update: bytes[HEADER..].to_vec(),
    })
}
fn invalid(message: &str) -> FileError {
    FileError::Io(message.to_owned())
}
fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>, FileError> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(FileError::TooLarge);
    }
    Ok(bytes)
}
fn optional_read(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, FileError> {
    if path.is_symlink() {
        return Err(invalid("Collaboration metadata must not be a symlink"));
    }
    match bounded_read(path, limit) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(FileError::Io(_)) if !path.try_exists()? => Ok(None),
        Err(error) => Err(error),
    }
}
struct Paths {
    markdown: PathBuf,
    state: PathBuf,
    journal: PathBuf,
    lock: PathBuf,
}
impl Paths {
    fn new(path: &Path) -> Result<Self, FileError> {
        let markdown = if path.is_symlink() {
            fs::canonicalize(path)?
        } else {
            path.to_owned()
        };
        let filename = markdown
            .file_name()
            .ok_or_else(|| invalid("Missing filename"))?;
        let sibling = |suffix: &str| {
            let mut name = OsString::from(".");
            name.push(filename);
            // Retain the checkpoint and lock namespace across the Dosty rename.
            name.push(".daytrip");
            name.push(suffix);
            markdown.with_file_name(name)
        };
        Ok(Self {
            state: sibling(""),
            journal: sibling("-journal"),
            lock: sibling("-lock"),
            markdown,
        })
    }
    fn acquire(&self) -> Result<File, FileError> {
        if self.lock.is_symlink() {
            return Err(invalid("Collaboration lock must not be a symlink"));
        }
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(&self.lock)?;
        file.lock()?;
        Ok(file)
    }
    fn read_lock(&self) -> Result<Option<File>, FileError> {
        if self.lock.is_symlink() {
            return Err(invalid("Collaboration lock must not be a symlink"));
        }
        match File::open(&self.lock) {
            Ok(file) => {
                file.lock_shared()?;
                Ok(Some(file))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    fn sync_parent(&self) -> Result<(), FileError> {
        #[cfg(unix)]
        File::open(self.markdown.parent().unwrap_or(Path::new(".")))?.sync_all()?;
        Ok(())
    }
    fn recover(&self) -> Result<(), FileError> {
        let Some(journal) = optional_read(&self.journal, MAX_STATE_BYTES as u64 + 136)? else {
            return Ok(());
        };
        if journal.len() < 72 || &journal[..8] != JOURNAL {
            return Err(invalid(
                "Damaged Dosty save journal; preserve files for recovery",
            ));
        }
        let state = decode(&journal[72..])?;
        let (_, source) =
            Collaboration::from_state(&state).map_err(|error| invalid(&error.to_string()))?;
        let markdown = optional_read(&self.markdown, MAX_FILE_BYTES)?;
        let actual = markdown
            .as_deref()
            .map(blake3::hash)
            .map(|hash| *hash.as_bytes())
            .unwrap_or([0; 32]);
        let successor = blake3::hash(source.as_bytes());
        if actual != journal[8..40] && actual != *successor.as_bytes() {
            return Err(FileError::RecoveryConflict);
        }
        let previous = optional_read(&self.state, MAX_STATE_BYTES as u64 + HEADER as u64)?;
        let previous_hash = previous
            .as_deref()
            .map(blake3::hash)
            .map(|hash| *hash.as_bytes())
            .unwrap_or([0; 32]);
        if previous_hash != journal[40..72] && previous.as_deref() != Some(&journal[72..]) {
            return Err(FileError::RecoveryConflict);
        }
        if actual != *successor.as_bytes() {
            atomic_write(&self.markdown, source.as_bytes())?;
        }
        atomic_write(&self.state, &journal[72..])?;
        fs::remove_file(&self.journal)?;
        self.sync_parent()
    }
}

pub fn load(path: &Path) -> Result<(String, Option<ReplicaState>), FileError> {
    let paths = Paths::new(path)?;
    // Ordinary/read-only Markdown opens need no directory write permission.
    // A journal needs an exclusive writer lock to perform recovery.
    let _lock = if paths.journal.try_exists()? {
        let lock = paths.acquire()?;
        paths.recover()?;
        Some(lock)
    } else {
        paths.read_lock()?
    };
    if paths.journal.try_exists()? {
        return Err(FileError::RecoveryConflict);
    }
    let source = String::from_utf8(bounded_read(&paths.markdown, MAX_FILE_BYTES)?)
        .map_err(|_| invalid("This is not a UTF-8 text file"))?;
    let state = optional_read(&paths.state, MAX_STATE_BYTES as u64 + HEADER as u64)?
        .map(|bytes| decode(&bytes))
        .transpose()?;
    if state.is_none() && _lock.is_some() {
        return Err(FileError::MissingCheckpoint);
    }
    if let Some(state) = &state {
        let (_, materialized) =
            Collaboration::from_state(state).map_err(|error| invalid(&error.to_string()))?;
        if source != materialized {
            return Err(FileError::MetadataConflict);
        }
    }
    if paths.journal.try_exists()? {
        return Err(FileError::RecoveryConflict);
    }
    Ok((source, state))
}

pub fn save(
    path: &Path,
    source: &str,
    state: &ReplicaState,
    expected: Option<&str>,
    expected_version: Option<&str>,
) -> Result<(), FileError> {
    let paths = Paths::new(path)?;
    let _lock = paths.acquire()?;
    paths.recover()?;
    let current = optional_read(&paths.markdown, MAX_FILE_BYTES)?;
    if let Some(expected) = expected
        && current.as_deref() != Some(expected.as_bytes())
    {
        return Err(FileError::Conflict);
    }
    let previous = optional_read(&paths.state, MAX_STATE_BYTES as u64 + HEADER as u64)?;
    match (&previous, expected_version) {
        (Some(bytes), Some(expected_version)) => {
            let previous = decode(bytes)?;
            if previous.document != state.document || version(&previous) != expected_version {
                return Err(FileError::MetadataConflict);
            }
        }
        (None, None) => {}
        _ => return Err(FileError::MetadataConflict),
    }
    let (_, materialized) =
        Collaboration::from_state(state).map_err(|error| invalid(&error.to_string()))?;
    if materialized != source {
        return Err(invalid("Save snapshot source differs from CRDT"));
    }
    let checkpoint = encode(state)?;
    let mut journal = Vec::with_capacity(72 + checkpoint.len());
    journal.extend_from_slice(JOURNAL);
    journal.extend_from_slice(
        &current
            .as_deref()
            .map(blake3::hash)
            .map(|hash| *hash.as_bytes())
            .unwrap_or([0; 32]),
    );
    journal.extend_from_slice(
        &previous
            .as_deref()
            .map(blake3::hash)
            .map(|hash| *hash.as_bytes())
            .unwrap_or([0; 32]),
    );
    journal.extend_from_slice(&checkpoint);
    atomic_write(&paths.journal, &journal)?;
    atomic_write(&paths.markdown, source.as_bytes())?;
    atomic_write(&paths.state, &checkpoint)?;
    fs::remove_file(&paths.journal)?;
    paths.sync_parent()
}

pub async fn save_document(
    path: PathBuf,
    source: String,
    state: ReplicaState,
    expected: Option<String>,
    expected_version: Option<String>,
) -> Result<PathBuf, FileError> {
    tokio::task::spawn_blocking(move || {
        save(
            &path,
            &source,
            &state,
            expected.as_deref(),
            expected_version.as_deref(),
        )?;
        Ok(path)
    })
    .await
    .map_err(|error| invalid(&error.to_string()))?
}

pub async fn write_replica(path: PathBuf, state: ReplicaState) -> Result<(), FileError> {
    tokio::task::spawn_blocking(move || atomic_write(&path, &encode(&state)?))
        .await
        .map_err(|error| invalid(&error.to_string()))?
}
pub async fn read_replica(path: PathBuf) -> Result<ReplicaState, FileError> {
    tokio::task::spawn_blocking(move || {
        decode(&bounded_read(
            &path,
            MAX_STATE_BYTES as u64 + HEADER as u64,
        )?)
    })
    .await
    .map_err(|error| invalid(&error.to_string()))?
}
pub async fn write_snapshot(
    path: PathBuf,
    snapshot: crate::document::collaboration::AgentSnapshot,
) -> Result<(), FileError> {
    tokio::task::spawn_blocking(move || {
        let bytes = serde_json::to_vec(&snapshot).map_err(|error| invalid(&error.to_string()))?;
        if bytes.len() > MAX_STATE_BYTES * 2 + 2 * MAX_FILE_BYTES as usize {
            return Err(FileError::TooLarge);
        }
        atomic_write(&path, &bytes)
    })
    .await
    .map_err(|error| invalid(&error.to_string()))?
}
pub async fn read_proposal(
    path: PathBuf,
) -> Result<crate::document::collaboration::AgentProposal, FileError> {
    tokio::task::spawn_blocking(move || {
        let bytes = bounded_read(
            &path,
            (MAX_STATE_BYTES * 2 + 2 * MAX_FILE_BYTES as usize) as u64,
        )?;
        serde_json::from_slice(&bytes).map_err(|error| invalid(&error.to_string()))
    })
    .await
    .map_err(|error| invalid(&error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    #[test]
    fn daytrip_checkpoints_and_locks_remain_compatible_with_dosty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        let document = Document::new("Existing draft 日本 🦀\r\n".into());
        let state = document.collaboration.state();
        fs::write(&path, document.source()).unwrap();
        fs::write(
            directory.path().join(".note.md.daytrip"),
            encode(&state).unwrap(),
        )
        .unwrap();

        let (source, restored) = load(&path).unwrap();
        assert_eq!(source, document.source());
        let restored = restored.expect("the existing checkpoint must not become a new import");
        assert_eq!(restored.document, state.document);
        assert_eq!(restored.update, state.update);

        let held = Paths::new(&path).unwrap().acquire().unwrap();
        let legacy = OpenOptions::new()
            .read(true)
            .write(true)
            .open(directory.path().join(".note.md.daytrip-lock"))
            .unwrap();
        assert!(legacy.try_lock().is_err());
        drop(held);
        legacy.try_lock().unwrap();
    }

    fn stage(paths: &Paths, source: &str, state: &ReplicaState) {
        let mut journal = Vec::from(JOURNAL.as_slice());
        journal.extend_from_slice(
            &optional_read(&paths.markdown, MAX_FILE_BYTES)
                .unwrap()
                .as_deref()
                .map(blake3::hash)
                .map(|hash| *hash.as_bytes())
                .unwrap_or([0; 32]),
        );
        journal.extend_from_slice(
            &optional_read(&paths.state, MAX_STATE_BYTES as u64 + HEADER as u64)
                .unwrap()
                .as_deref()
                .map(blake3::hash)
                .map(|hash| *hash.as_bytes())
                .unwrap_or([0; 32]),
        );
        journal.extend_from_slice(&encode(state).unwrap());
        atomic_write(&paths.journal, &journal).unwrap();
        assert_eq!(Collaboration::from_state(state).unwrap().1, source);
    }
    #[test]
    fn interruptions_at_each_publication_boundary_recover_one_consistent_generation() {
        for phase in 0..3 {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("note.md");
            let mut document = Document::new("Old 🦀\r\n".into());
            let old = document.collaboration.state();
            save(&path, document.source(), &old, None, None).unwrap();
            document.replace(0..3, "New 日本").unwrap();
            let new = document.collaboration.state();
            let paths = Paths::new(&path).unwrap();
            stage(&paths, document.source(), &new);
            if phase > 0 {
                atomic_write(&path, document.source().as_bytes()).unwrap();
            }
            if phase > 1 {
                atomic_write(&paths.state, &encode(&new).unwrap()).unwrap();
            }
            let (source, state) = load(&path).unwrap();
            assert_eq!(source, document.source());
            assert_eq!(state.unwrap().update, new.update);
            assert!(!paths.journal.exists());
            let reopened = Document::from_state(&load(&path).unwrap().1.unwrap()).unwrap();
            assert_ne!(
                reopened.collaboration.replica_id(),
                document.collaboration.replica_id()
            );
            assert_eq!(reopened.collaboration.id(), document.collaboration.id());
            assert_eq!(reopened.source(), document.source());
        }
    }
    #[test]
    fn external_markdown_stale_metadata_and_foreign_identity_are_explicit_conflicts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        let mut document = Document::new("original".into());
        let original = document.collaboration.state();
        save(&path, document.source(), &original, None, None).unwrap();
        let baseline = version(&original);
        document.replace(0..8, "updated").unwrap();
        let updated = document.collaboration.state();
        fs::write(&path, "external").unwrap();
        assert!(matches!(load(&path), Err(FileError::MetadataConflict)));
        assert!(matches!(
            save(
                &path,
                document.source(),
                &updated,
                Some("original"),
                Some(&baseline)
            ),
            Err(FileError::Conflict)
        ));
        fs::write(&path, "original").unwrap();
        assert!(matches!(
            save(
                &path,
                document.source(),
                &updated,
                Some("original"),
                Some("stale")
            ),
            Err(FileError::MetadataConflict)
        ));
        let foreign = Document::new("original".into()).collaboration.state();
        assert!(matches!(
            save(
                &path,
                "original",
                &foreign,
                Some("original"),
                Some(&baseline)
            ),
            Err(FileError::MetadataConflict)
        ));
        stage(&Paths::new(&path).unwrap(), document.source(), &updated);
        fs::write(&path, "external after crash").unwrap();
        assert!(matches!(load(&path), Err(FileError::RecoveryConflict)));
        assert_eq!(fs::read_to_string(path).unwrap(), "external after crash");
    }
    #[test]
    fn checkpoint_checksum_size_and_symlink_guards_preserve_files() {
        let document = Document::new("# 日本\r\n\ntext\rmore\n".into());
        let state = document.collaboration.state();
        let encoded = encode(&state).unwrap();
        for end in [0, 7, 63, encoded.len() - 1] {
            assert!(decode(&encoded[..end]).is_err());
        }
        let mut corrupted = encoded.clone();
        *corrupted.last_mut().unwrap() ^= 1;
        assert!(decode(&corrupted).is_err());
        let mut wrong_identity = encoded.clone();
        wrong_identity[8] ^= 1;
        assert!(decode(&wrong_identity).is_err());
        let mut huge = encoded.clone();
        huge[56..64].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(decode(&huge).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt as _, symlink};
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("note.md");
            let link = directory.path().join("link.md");
            fs::write(&path, "old").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
            symlink(&path, &link).unwrap();
            save(&link, document.source(), &state, Some("old"), None).unwrap();
            assert!(link.is_symlink());
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o640
            );
            let paths = Paths::new(&path).unwrap();
            assert_eq!(
                fs::metadata(&paths.state).unwrap().permissions().mode() & 0o777,
                0o600
            );
            fs::remove_file(&paths.state).unwrap();
            symlink(&path, &paths.state).unwrap();
            assert!(load(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), document.source());
        }
    }
    #[test]
    fn pending_updates_and_causal_only_saves_survive_reopen() {
        use yrs::{ReadTxn, Transact};
        let mut a = Document::new("base".into());
        let mut b = Document::from_state(&a.collaboration.state()).unwrap();
        let clock = b.collaboration.doc_for_test().transact().state_vector();
        b.replace(4..4, " first").unwrap();
        let first = b.collaboration.diff_for_test(&clock);
        let clock = b.collaboration.doc_for_test().transact().state_vector();
        b.replace(10..10, " second").unwrap();
        let second = b.collaboration.diff_for_test(&clock);
        a.receive(&second).unwrap();
        assert_eq!(a.source(), "base");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        save(&path, a.source(), &a.collaboration.state(), None, None).unwrap();
        let mut reopened = Document::from_state(&load(&path).unwrap().1.unwrap()).unwrap();
        reopened.receive(&first).unwrap();
        assert_eq!(reopened.source(), b.source());
    }
}

#[cfg(test)]
mod writer_tests {
    use super::*;
    use crate::document::Document;
    #[test]
    fn independent_writers_detect_checkpoint_conflicts_under_the_same_file_lock() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        let document = Document::new("initial".into());
        let state = document.collaboration.state();
        save(&path, document.source(), &state, None, None).unwrap();
        let version = version(&state);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = ["first", "other"]
            .into_iter()
            .map(|text| {
                let mut replica = Document::from_state(&state).unwrap();
                replica.replace(0..7, text).unwrap();
                let state = replica.collaboration.state();
                let path = path.clone();
                let version = version.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    save(&path, text, &state, Some("initial"), Some(&version))
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    Err(FileError::Conflict | FileError::MetadataConflict)
                ))
                .count(),
            1
        );
        let (source, state) = load(&path).unwrap();
        assert_eq!(
            Collaboration::from_state(&state.unwrap()).unwrap().1,
            source
        );
    }
}

#[cfg(test)]
mod missing_checkpoint_tests {
    use super::*;
    use crate::document::Document;
    #[test]
    fn a_missing_checkpoint_does_not_silently_reset_a_known_local_history() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        let document = Document::new("preserve me".into());
        save(
            &path,
            document.source(),
            &document.collaboration.state(),
            None,
            None,
        )
        .unwrap();
        fs::remove_file(Paths::new(&path).unwrap().state).unwrap();
        assert!(matches!(load(&path), Err(FileError::MissingCheckpoint)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "preserve me");
        let copy = directory.path().join("explicit-import.md");
        fs::copy(&path, &copy).unwrap();
        assert!(load(&copy).unwrap().1.is_none());
    }
}
