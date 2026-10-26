//! File operations run off the UI thread. Saves are atomic and detect external changes.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Error)]
pub enum FileError {
    #[error("{0}")]
    Io(String),
    #[error("This file changed outside Daytrip. Use Save As to keep both versions.")]
    Conflict,
    #[error(
        "Markdown differs from its collaboration checkpoint. Preserve both files; open a copy without metadata to import it as a new document."
    )]
    MetadataConflict,
    #[error(
        "The collaboration checkpoint is missing. Restore it, or import a separate Markdown copy to explicitly start a new history."
    )]
    MissingCheckpoint,
    #[error(
        "An interrupted save conflicts with external changes. Preserve the Markdown, .daytrip, and .daytrip-journal files for recovery."
    )]
    RecoveryConflict,
    #[error("This file exceeds Daytrip's current 16 MiB document limit.")]
    TooLarge,
}

impl From<std::io::Error> for FileError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct LoadedFile {
    pub path: PathBuf,
    pub source: String,
    pub collaboration: Option<crate::document::collaboration::ReplicaState>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub theme: String,
    pub custom_theme: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
}

pub fn preferences_path() -> Option<PathBuf> {
    if let Some(directory) = std::env::var_os("DAYTRIP_CONFIG_DIR") {
        return Some(PathBuf::from(directory).join("preferences.toml"));
    }
    // Native platform paths follow the public application identity.
    ProjectDirs::from("app", "Daytrip", "Daytrip")
        .map(|dirs| dirs.config_dir().join("preferences.toml"))
}

pub fn load_preferences() -> Preferences {
    preferences_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

pub async fn save_preferences(preferences: Preferences) -> Result<(), FileError> {
    let Some(path) = preferences_path() else {
        return Ok(());
    };
    let source =
        toml::to_string_pretty(&preferences).map_err(|error| FileError::Io(error.to_string()))?;
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write(&path, source.as_bytes())
    })
    .await
    .map_err(|error| FileError::Io(error.to_string()))?
}

pub async fn load(path: PathBuf) -> Result<LoadedFile, FileError> {
    tokio::task::spawn_blocking(move || {
        let (source, collaboration) = crate::io::collaboration_store::load(&path)?;
        Ok(LoadedFile {
            path,
            source,
            collaboration,
        })
    })
    .await
    .map_err(|error| FileError::Io(error.to_string()))?
}

pub async fn save(
    path: PathBuf,
    source: String,
    expected: Option<String>,
) -> Result<PathBuf, FileError> {
    tokio::task::spawn_blocking(move || {
        if let Some(expected) = expected {
            match fs::read_to_string(&path) {
                Ok(actual) if actual == expected => {}
                _ => return Err(FileError::Conflict),
            }
        }
        atomic_write(&path, source.as_bytes())?;
        Ok(path)
    })
    .await
    .map_err(|error| FileError::Io(error.to_string()))?
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), FileError> {
    // Resolve symlinks so a save updates the target, rather than replacing the link.
    let target = if path.is_symlink() {
        fs::canonicalize(path)?
    } else {
        path.to_path_buf()
    };
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    if let Ok(metadata) = fs::metadata(&target) {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(&target)
        .map_err(|error| FileError::Io(error.error.to_string()))?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
fn export_html(source: &str) -> String {
    render_html(
        pulldown_cmark::Parser::new_ext(source, pulldown_cmark::Options::all()),
        "Daytrip — document",
    )
}

pub(super) fn render_html<'a>(
    events: impl Iterator<Item = pulldown_cmark::Event<'a>>,
    title: &str,
) -> String {
    let mut body = String::new();
    // Markdown HTML is escaped in export so a note cannot embed executable scripts.
    let events = events.map(|event| match event {
        pulldown_cmark::Event::Html(html) | pulldown_cmark::Event::InlineHtml(html) => {
            pulldown_cmark::Event::Text(html)
        }
        other => other,
    });
    pulldown_cmark::html::push_html(&mut body, events);
    let title = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    // Export stays in Daytrip's light reading palette, independent of the
    // current editing atmosphere. Resolve here on the background export path.
    let paper = crate::theme::Preset::Paper.resolve();
    let hex = |color: iced::Color| {
        let [r, g, b, _] = color.into_rgba8();
        format!("#{r:02x}{g:02x}{b:02x}")
    };
    let ink = hex(paper.tokens.text);
    let canvas = hex(paper.tokens.canvas);
    let accent = hex(paper.tokens.accent);
    let code = hex(paper.tokens.code);
    let muted = hex(paper.tokens.muted);
    let border = hex(paper.tokens.border);
    let selection = hex(paper.tokens.selection);
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src http: https: data:; style-src 'unsafe-inline'\"><title>{title}</title><style>body{{max-width:720px;margin:64px auto;padding:0 24px;color:{ink};background:{canvas};font:18px/1.7 Georgia,serif}}h1,h2,h3{{line-height:1.25}}a{{color:{accent}}}pre,code{{background:{code};border-radius:5px}}pre{{padding:20px;overflow:auto}}blockquote{{border-left:3px solid {accent};margin-left:0;padding-left:24px;color:{muted}}}table{{border-collapse:collapse;width:100%}}td,th{{border:1px solid {border};padding:8px 12px}}img{{display:block;max-width:100%;max-height:70vh;width:auto;height:auto;margin:24px auto 12px;border-radius:4px}}p{{margin:0 0 1.1em}}h1{{font-size:2.22em;margin-bottom:.55em}}h2{{margin-top:1.4em}}blockquote p:last-child{{font-size:.92em}}::selection{{background:{selection}}}a:focus-visible{{outline:2px solid {accent};outline-offset:3px}}@media(max-width:600px){{body{{margin:32px auto;padding:0 20px}}h1{{font-size:1.9em}}}}@media print{{body{{margin:0 auto;background:white}}a{{color:inherit}}img{{max-height:65vh}}}}</style></head><body>\n{body}\n</body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::{FileError, atomic_write, export_html};

    #[test]
    fn atomic_save_replaces_exact_bytes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("note.md");
        atomic_write(&path, b"old").expect("initial save");
        atomic_write(&path, "日本語\r\n".as_bytes()).expect("replacement save");
        assert_eq!(
            std::fs::read_to_string(path).expect("saved file"),
            "日本語\r\n"
        );
    }

    #[tokio::test]
    async fn detects_external_changes_before_overwriting() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("note.md");
        atomic_write(&path, b"external change").expect("initial save");
        assert!(matches!(
            super::save(path.clone(), "ours".into(), Some("old".into())).await,
            Err(FileError::Conflict)
        ));
        assert_eq!(
            std::fs::read_to_string(path).expect("saved file"),
            "external change"
        );
    }

    #[test]
    fn html_export_escapes_raw_scripts() {
        let html = export_html("# Hello\n\n<script>alert(1)</script>");
        assert!(html.contains("<h1>Hello</h1>"));
        assert!(!html.contains("<script>"));
    }
}
