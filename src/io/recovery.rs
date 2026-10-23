//! Idle draft checkpoints are independent of saved-file publication. A single
//! checksummed atomic file contains the compatible CRDT state and saved baseline.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::document::collaboration::{MAX_STATE_BYTES, ReplicaState};
use crate::io::storage::{FileError, MAX_FILE_BYTES, atomic_write};

const MAGIC: &[u8; 8] = b"STILDR01";
const LIMIT: u64 = 112 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub state: ReplicaState,
    pub path: Option<PathBuf>,
    pub name: String,
    pub baseline: String,
    pub baseline_version: Option<String>,
    pub anchor: usize,
    pub head: usize,
}
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: Uuid,
    pub name: String,
    pub modified: u64,
}
fn path(root: &Path, id: Uuid) -> PathBuf {
    root.join(format!("{id}.stilldraft"))
}
fn invalid(message: &str) -> FileError {
    FileError::Io(message.into())
}

pub fn write(root: &Path, id: Uuid, snapshot: &Snapshot) -> Result<(), FileError> {
    if snapshot.state.update.len() > MAX_STATE_BYTES
        || snapshot.baseline.len() as u64 > MAX_FILE_BYTES
        || snapshot.name.len() > 1024
    {
        return Err(invalid(
            "Recovery copy exceeds the 16 MiB source, 64 MiB state, or 112 MiB checkpoint limit.",
        ));
    }
    let body = serde_json::to_vec(snapshot).map_err(|e| invalid(&e.to_string()))?;
    let mut bytes = Vec::with_capacity(body.len() + 1066);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(snapshot.name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(snapshot.name.as_bytes());
    bytes.extend_from_slice(blake3::hash(&body).as_bytes());
    bytes.extend_from_slice(&body);
    if bytes.len() as u64 > LIMIT {
        return Err(invalid(
            "Recovery copy exceeds the 16 MiB source, 64 MiB state, or 112 MiB checkpoint limit.",
        ));
    }
    fs::create_dir_all(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    if path(root, id).is_symlink() {
        return Err(invalid("Recovery copies must not be symlinks."));
    }
    atomic_write(&path(root, id), &bytes)
}
fn header(file: &mut File) -> Result<String, FileError> {
    let mut header = [0; 10];
    file.read_exact(&mut header)?;
    if &header[..8] != MAGIC {
        return Err(invalid("Unsupported recovery copy."));
    }
    let length = u16::from_le_bytes([header[8], header[9]]) as usize;
    if length > 1024 {
        return Err(invalid("Invalid recovery header."));
    }
    let mut name = vec![0; length];
    file.read_exact(&mut name)?;
    String::from_utf8(name).map_err(|e| invalid(&e.to_string()))
}
pub fn read(root: &Path, id: Uuid) -> Result<Snapshot, FileError> {
    let path = path(root, id);
    if path.is_symlink() {
        return Err(invalid("Recovery copies must not be symlinks."));
    }
    let mut file = File::open(path)?;
    if file.metadata()?.len() > LIMIT {
        return Err(invalid(
            "Recovery copy exceeds the 16 MiB source, 64 MiB state, or 112 MiB checkpoint limit.",
        ));
    }
    let name = header(&mut file)?;
    let mut checksum = [0; 32];
    file.read_exact(&mut checksum)?;
    let mut body = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut body)?;
    if body.len() as u64 > LIMIT {
        return Err(invalid(
            "Recovery copy exceeds the 16 MiB source, 64 MiB state, or 112 MiB checkpoint limit.",
        ));
    }
    if blake3::hash(&body).as_bytes() != &checksum {
        return Err(invalid(
            "Recovery checksum mismatch. The copy was left untouched.",
        ));
    }
    let snapshot: Snapshot = serde_json::from_slice(&body).map_err(|e| invalid(&e.to_string()))?;
    if snapshot.name != name
        || snapshot.state.update.len() > MAX_STATE_BYTES
        || snapshot.baseline.len() as u64 > MAX_FILE_BYTES
    {
        return Err(invalid("Invalid recovery copy."));
    }
    Ok(snapshot)
}
pub fn list(root: &Path) -> Result<Vec<Entry>, FileError> {
    let mut entries = Vec::new();
    let directory = match fs::read_dir(root) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(entries),
        Err(e) => return Err(e.into()),
    };
    for entry in directory {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_none_or(|ext| ext != "stilldraft")
            || entry.path().is_symlink()
        {
            continue;
        }
        let Some(id) = entry
            .path()
            .file_stem()
            .and_then(|name| name.to_str())
            .and_then(|name| Uuid::parse_str(name).ok())
        else {
            continue;
        };
        let mut file = File::open(entry.path())?;
        // A damaged entry does not hide the other recovery copies.
        if let Ok(name) = header(&mut file) {
            let modified = file
                .metadata()?
                .modified()?
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            entries.push(Entry { id, name, modified });
        }
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.modified));
    Ok(entries)
}
pub fn remove(root: &Path, id: Uuid) -> Result<(), FileError> {
    match fs::remove_file(path(root, id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    #[test]
    fn atomic_recovery_retains_baseline_and_reopens_with_a_fresh_writer() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let doc = Document::new("# Draft\r\n\r\n海 👩‍👩‍👧‍👦 e\u{301}\n".into());
        let snapshot = Snapshot {
            state: doc.collaboration.state(),
            path: Some(dir.path().join("note.md")),
            name: "note.md".into(),
            baseline: "on disk".into(),
            baseline_version: Some("digest".into()),
            anchor: 12,
            head: 4,
        };
        write(dir.path(), id, &snapshot).unwrap();
        let restored = read(dir.path(), id).unwrap();
        let reopened = Document::from_state(&restored.state).unwrap();
        assert_eq!(reopened.source(), doc.source());
        assert_ne!(
            reopened.collaboration.replica_id(),
            doc.collaboration.replica_id()
        );
        assert_eq!(restored.baseline, "on disk");
        assert_eq!(restored.anchor, 12);
        // An abandoned atomic-write temporary does not replace the last valid copy.
        fs::write(dir.path().join(".interrupted"), b"partial").unwrap();
        assert_eq!(list(dir.path()).unwrap().len(), 1);
        let mut bytes = fs::read(path(dir.path(), id)).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(path(dir.path(), id), bytes).unwrap();
        assert!(read(dir.path(), id).is_err());
        remove(dir.path(), id).unwrap();
        assert!(list(dir.path()).unwrap().is_empty());
    }
}
