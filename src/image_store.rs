//! Bounded image IO and decoding, kept off the GUI thread.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use iced::advanced::image::Handle;
use iced::{Size, Task};

use crate::app::Message;
use crate::rich_document::Image;

const MAX_ENCODED_BYTES: usize = 16 * 1024 * 1024;
const MAX_DECODED_BYTES: u64 = 64 * 1024 * 1024;
const CACHE_BYTES: usize = 128 * 1024 * 1024;
const CONCURRENT_LOADS: usize = 4;
static LOADS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(CONCURRENT_LOADS);
static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct LoadedImage {
    pub handle: Handle,
    pub size: Size<u32>,
    bytes: usize,
}

pub enum Asset {
    Loading,
    Ready(LoadedImage),
    Failed(String),
}

#[derive(Default)]
pub struct ImageStore {
    assets: HashMap<String, Asset>,
    revision: u64,
}

impl ImageStore {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn get(&self, url: &str, base: Option<&Path>) -> Option<&Asset> {
        self.assets.get(&key(url, base))
    }

    pub fn loaded(&mut self, key: &str, result: Result<LoadedImage, String>) {
        if !self.assets.contains_key(key) {
            return;
        }
        let used = self
            .assets
            .values()
            .filter_map(|asset| match asset {
                Asset::Ready(image) => Some(image.bytes),
                _ => None,
            })
            .sum::<usize>();
        let asset = match result {
            Ok(image) if used + image.bytes <= CACHE_BYTES => Asset::Ready(image),
            Ok(_) => Asset::Failed("Document image memory limit reached".into()),
            Err(error) => Asset::Failed(error),
        };
        self.assets.insert(key.to_owned(), asset);
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn request(&mut self, images: &[Image], base: Option<&Path>) -> Task<Message> {
        let active = images
            .iter()
            .map(|image| key(&image.url, base))
            .collect::<HashSet<_>>();
        self.assets.retain(|key, _| active.contains(key));
        let loading = self
            .assets
            .values()
            .filter(|asset| matches!(asset, Asset::Loading))
            .count();
        let mut tasks = Vec::new();
        for image in images {
            if tasks.len() + loading >= CONCURRENT_LOADS {
                break;
            }
            let key = key(&image.url, base);
            if self.assets.contains_key(&key) {
                continue;
            }
            self.assets.insert(key.clone(), Asset::Loading);
            let url = image.url.clone();
            let base = base.map(Path::to_path_buf);
            tasks.push(Task::perform(load(url, base), move |result| {
                Message::ImageLoaded {
                    key: key.clone(),
                    result,
                }
            }));
        }
        Task::batch(tasks)
    }
}

fn key(url: &str, base: Option<&Path>) -> String {
    if url.starts_with("https://") || url.starts_with("http://") || url.starts_with("file://") {
        return url.to_owned();
    }
    format!(
        "{}\0{url}",
        base.map_or_else(String::new, |path| path.to_string_lossy().into_owned())
    )
}

async fn load(url: String, base: Option<PathBuf>) -> Result<LoadedImage, String> {
    let _permit = LOADS.acquire().await.map_err(|error| error.to_string())?;
    let bytes = if url.starts_with("https://") || url.starts_with("http://") {
        let client = CLIENT
            .get_or_init(|| {
                reqwest::Client::builder()
                    .timeout(Duration::from_secs(20))
                    .redirect(reqwest::redirect::Policy::limited(5))
                    .build()
                    .map_err(|error| error.to_string())
            })
            .as_ref()
            .map_err(Clone::clone)?;
        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ENCODED_BYTES as u64)
        {
            return Err("Image exceeds 16 MiB".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
            if bytes.len() + chunk.len() > MAX_ENCODED_BYTES {
                return Err("Image exceeds 16 MiB".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        bytes
    } else {
        let path = if url.starts_with("file://") {
            reqwest::Url::parse(&url)
                .map_err(|error| error.to_string())?
                .to_file_path()
                .map_err(|()| "Invalid file URL".to_owned())?
        } else if url.contains("://") || url.starts_with("data:") {
            return Err("Unsupported image URL".into());
        } else {
            let decoded = percent_encoding::percent_decode_str(&url)
                .decode_utf8()
                .map_err(|error| error.to_string())?;
            let path = Path::new(decoded.as_ref());
            if path.is_relative() && base.is_none() {
                return Err("Save this document to resolve relative images".into());
            }
            base.as_deref().unwrap_or_else(|| Path::new(".")).join(path)
        };
        tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
            let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
            let mut bytes = Vec::new();
            file.take(MAX_ENCODED_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            if bytes.len() > MAX_ENCODED_BYTES {
                return Err("Image exceeds 16 MiB".into());
            }
            Ok(bytes)
        })
        .await
        .map_err(|error| error.to_string())??
    };
    tokio::task::spawn_blocking(move || decode(&bytes))
        .await
        .map_err(|error| error.to_string())?
}

fn decode(bytes: &[u8]) -> Result<LoadedImage, String> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| error.to_string())?;
    let image = if image.width() > 4096 || image.height() > 4096 {
        image.thumbnail(4096, 4096)
    } else {
        image
    };
    let rgba = image.into_rgba8();
    let size = Size::new(rgba.width(), rgba.height());
    let pixels = rgba.into_raw();
    Ok(LoadedImage {
        bytes: pixels.len(),
        handle: Handle::from_rgba(size.width, size.height, pixels),
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            8,
            4,
            image::Rgba([70, 100, 90, 255]),
        ));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[tokio::test]
    async fn local_images_decode_relative_percent_encoded_and_file_urls() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("quiet room.png");
        std::fs::write(&path, png()).unwrap();
        for (url, base) in [
            (
                "quiet%20room.png".to_owned(),
                Some(directory.path().to_owned()),
            ),
            (
                reqwest::Url::from_file_path(&path).unwrap().to_string(),
                None,
            ),
        ] {
            let image = load(url, base).await.unwrap();
            assert_eq!(image.size, Size::new(8, 4));
            assert_eq!(image.bytes, 128);
        }
        assert!(
            load("relative.png".into(), None)
                .await
                .unwrap_err()
                .contains("Save this document")
        );
        assert!(decode(b"broken image").is_err());
    }

    #[tokio::test]
    async fn http_images_and_response_limits_are_checked() {
        use std::io::{BufRead, Write};
        use std::net::TcpListener;
        let image = png();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/image.png", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for oversized in [false, true] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(&stream);
                let mut line = String::new();
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                }
                let length = if oversized {
                    MAX_ENCODED_BYTES + 1
                } else {
                    image.len()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                if !oversized {
                    stream.write_all(&image).unwrap();
                }
            }
        });
        assert_eq!(load(url.clone(), None).await.unwrap().size, Size::new(8, 4));
        assert!(load(url, None).await.unwrap_err().contains("16 MiB"));
        server.join().unwrap();
    }
}
