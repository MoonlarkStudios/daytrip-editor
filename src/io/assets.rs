//! Bounded, immutable local assets. Call from a blocking worker, never the UI.

use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use image::{ImageFormat, ImageReader};
use pulldown_cmark::{Event, Options, Parser, Tag};

use crate::io::storage::FileError;

pub const MAX_ENCODED: usize = 16 * 1024 * 1024;
const MAX_DECODED: u64 = 64 * 1024 * 1024;

/// Bounded local reference history, independent of the decoded-image cache.
#[derive(Debug, Clone)]
pub struct History {
    pub references: Vec<String>,
    pub complete: bool,
}
impl Default for History {
    fn default() -> Self {
        Self {
            references: Vec::new(),
            complete: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Input {
    File(PathBuf),
    Png(Vec<u8>),
}
#[derive(Debug, Clone)]
pub enum Clipboard {
    Text(String),
    Image(Input),
    Empty,
}

pub fn clipboard(fallback: Option<String>) -> Result<Clipboard, String> {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(clipboard) => clipboard,
        Err(error) => {
            return fallback
                .map(Clipboard::Text)
                .ok_or_else(|| error.to_string());
        }
    };
    if let Ok(files) = clipboard.get().file_list() {
        if let [path] = files.as_slice()
            && is_image(path)
        {
            return Ok(Clipboard::Image(Input::File(path.clone())));
        }
        if files.iter().any(|path| is_image(path)) {
            return Err("Paste one image file at a time, or drop images individually.".into());
        }
    }
    match clipboard.get_image() {
        Ok(image) => {
            let expected = image
                .width
                .checked_mul(image.height)
                .and_then(|n| n.checked_mul(4));
            if image.width > 8192
                || image.height > 8192
                || expected.is_none_or(|n| n > MAX_DECODED as usize || n != image.bytes.len())
            {
                return Err(
                    "The clipboard image exceeds the 64 MiB / 8192-pixel image limit.".into(),
                );
            }
            let image = image::RgbaImage::from_raw(
                image.width as u32,
                image.height as u32,
                image.bytes.into_owned(),
            )
            .ok_or("Invalid clipboard image")?;
            let mut bytes = Cursor::new(Vec::new());
            image
                .write_to(&mut bytes, ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            if bytes.get_ref().len() > MAX_ENCODED {
                return Err("The encoded clipboard image exceeds 16 MiB.".into());
            }
            Ok(Clipboard::Image(Input::Png(bytes.into_inner())))
        }
        Err(arboard::Error::ContentNotAvailable) => Ok(fallback
            .or_else(|| clipboard.get_text().ok())
            .map_or(Clipboard::Empty, Clipboard::Text)),
        Err(error) => fallback
            .map(Clipboard::Text)
            .ok_or_else(|| format!("Cannot read the clipboard: {error}")),
    }
}

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp"
            )
        })
}
pub fn read(path: &Path) -> Result<Vec<u8>, FileError> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_ENCODED as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_ENCODED {
        return Err(error("An image exceeds the 16 MiB encoded image limit."));
    }
    Ok(bytes)
}
pub fn format(bytes: &[u8]) -> Result<ImageFormat, FileError> {
    let format = image::guess_format(bytes).map_err(|e| error(&e.to_string()))?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP
    ) {
        return Err(error("Use PNG, JPEG, GIF, or WebP images."));
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(MAX_DECODED);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|e| error(&format!("Cannot decode image: {e}")))?;
    Ok(format)
}
pub fn base(document: &Path) -> Result<PathBuf, FileError> {
    fs::canonicalize(
        document
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
    .map_err(Into::into)
}
pub fn local_path(destination: &str, base: Option<&Path>) -> Result<Option<PathBuf>, FileError> {
    if destination.starts_with("http://") || destination.starts_with("https://") {
        return Ok(None);
    }
    if destination.starts_with("file://") {
        return reqwest::Url::parse(destination)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .map(Some)
            .ok_or_else(|| error("Invalid local image URL."));
    }
    if destination.contains(':') {
        return Err(error("Unsupported image destination."));
    }
    let decoded = percent_encoding::percent_decode_str(destination)
        .decode_utf8()
        .map_err(|e| error(&e.to_string()))?;
    let path = PathBuf::from(decoded.as_ref());
    if path.is_absolute() {
        Ok(Some(path))
    } else {
        Ok(Some(
            base.ok_or_else(|| error("Save the document to resolve relative images."))?
                .join(path),
        ))
    }
}

/// Create without replacing existing bytes, and refuse symlink components in the
/// destination. Keeping unused assets makes undo safe; cleanup is explicitly manual.
fn publish(base: &Path, relative: &Path, bytes: &[u8]) -> Result<(), FileError> {
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err(error(
            "Save As cannot relocate an image outside the document folder. Import it into Still first.",
        ));
    }
    let mut directory = base.to_owned();
    if let Some(parent) = relative.parent() {
        for part in parent.components() {
            directory.push(part);
            match fs::create_dir(&directory) {
                Ok(()) => {
                    #[cfg(unix)]
                    File::open(directory.parent().expect("asset parent"))?.sync_all()?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
            if directory.is_symlink() || !directory.is_dir() {
                return Err(error("The asset folder must be a real directory."));
            }
        }
    }
    let path = base.join(relative);
    // A temporary file with persist_noclobber avoids exposing a partial asset to
    // another process. Existing immutable content must agree byte for byte.
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(&path) {
        Ok(_) => {}
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            if path.is_symlink() || read(&path)? != bytes {
                return Err(error(
                    "An asset with this name already exists with different content. Nothing was overwritten.",
                ));
            }
        }
        Err(e) => return Err(e.error.into()),
    }
    #[cfg(unix)]
    File::open(&directory)?.sync_all()?;
    Ok(())
}

pub fn import(document: &Path, input: Input) -> Result<String, FileError> {
    let bytes = match input {
        Input::File(path) => read(&path)?,
        Input::Png(bytes) => bytes,
    };
    if bytes.len() > MAX_ENCODED {
        return Err(error("The image exceeds 16 MiB."));
    }
    let format = format(&bytes)?;
    let extension = match format {
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Gif => "gif",
        ImageFormat::WebP => "webp",
        _ => "png",
    };
    let folder = format!(
        "{}.assets",
        document.file_stem().unwrap_or_default().to_string_lossy()
    );
    let name = format!("{}.{extension}", blake3::hash(&bytes).to_hex());
    publish(&base(document)?, &Path::new(&folder).join(&name), &bytes)?;
    Ok(format!(
        "{}/{name}",
        percent_encoding::utf8_percent_encode(&folder, percent_encoding::NON_ALPHANUMERIC)
    ))
}

/// Preserve relative references verbatim on Save As. Copy into the chosen folder
/// before publishing Markdown; failure leaves the original document untouched.
pub fn relocate(
    source: &str,
    old: Option<&Path>,
    new: &Path,
    history: &History,
) -> Result<(), FileError> {
    // First save establishes the base of manually written relative references.
    // There is no earlier location to copy from; queued imports run after save.
    let Some(old) = old else {
        return Ok(());
    };
    let new_base = base(new)?;
    let old_base = Some(base(old)?);
    if old_base.as_ref() == Some(&new_base) {
        return Ok(());
    }
    let required = Parser::new_ext(source, Options::all())
        .filter_map(|event| match event {
            Event::Start(Tag::Image { dest_url, .. }) => Some(dest_url.into_string()),
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>();
    let mut references = required.clone();
    references.extend(history.references.iter().cloned());
    let mut references = references.into_iter().collect::<Vec<_>>();
    references.sort_unstable();
    if !history.complete {
        return Err(error(
            "Image reference history exceeds 16 MiB or 65,536 entries. Save and reopen the document to begin a new undo session before moving it to another folder.",
        ));
    }
    let mut total = 0;
    for destination in references {
        let path = match local_path(&destination, old_base.as_deref()) {
            Ok(Some(path)) => path,
            Ok(None) => continue,
            Err(_) if !required.contains(&destination) => continue,
            Err(error) => return Err(error),
        };
        let decoded = percent_encoding::percent_decode_str(&destination)
            .decode_utf8()
            .map_err(|e| error(&e.to_string()))?;
        let relative = Path::new(decoded.as_ref());
        if relative.is_absolute() || destination.starts_with("file://") {
            continue;
        }
        // A historical reference that was already broken need not block a copy.
        // Current references must all resolve; existing historical assets are
        // retained so Undo after Save As has the same resources as before it.
        if !required.contains(&destination) && !path.try_exists()? {
            continue;
        }
        let bytes = read(&path)?;
        total += bytes.len();
        if total > 128 * 1024 * 1024 {
            return Err(error(
                "Save As exceeds the 128 MiB asset-copy limit (including image undo history).",
            ));
        }
        format(&bytes)?;
        publish(&new_base, relative, &bytes)?;
    }
    Ok(())
}
fn error(message: &str) -> FileError {
    FileError::Io(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::new(4, 3)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    #[test]
    fn immutable_assets_survive_reimport_and_save_as_with_spaces() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let path = old.path().join("A quiet note.md");
        let destination = import(&path, Input::Png(png())).unwrap();
        assert_eq!(destination, import(&path, Input::Png(png())).unwrap());
        let source = format!("![alt](<{destination}>)");
        relocate(
            &source,
            Some(&path),
            &new.path().join("copy.md"),
            &History::default(),
        )
        .unwrap();
        assert_eq!(
            read(&local_path(&destination, Some(new.path())).unwrap().unwrap()).unwrap(),
            png()
        );
        relocate(
            &format!("![alt](<./{destination}>)"),
            Some(&path),
            &new.path().join("relative.md"),
            &History::default(),
        )
        .unwrap();
        assert!(import(&path, Input::Png(b"invalid".to_vec())).is_err());
        let incomplete = History {
            complete: false,
            ..History::default()
        };
        assert!(relocate(&source, Some(&path), &path, &incomplete).is_ok());
        assert!(
            relocate(
                &source,
                Some(&path),
                &new.path().join("blocked.md"),
                &incomplete
            )
            .is_err()
        );
        assert!(
            relocate(
                "![future](images/future.png)",
                None,
                &new.path().join("first.md"),
                &History::default(),
            )
            .is_ok()
        );
    }
    #[cfg(unix)]
    #[test]
    fn refuses_symlink_assets_and_parent_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("note.assets")).unwrap();
        assert!(import(&dir.path().join("note.md"), Input::Png(png())).is_err());
        assert!(publish(dir.path(), Path::new("../escape.png"), &png()).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
