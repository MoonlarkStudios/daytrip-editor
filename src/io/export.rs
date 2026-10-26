//! Self-contained HTML export. Files and decoding stay on a blocking worker.

use base64::Engine;
use pulldown_cmark::{Event, Parser, Tag};
use std::path::Path;

use crate::document::block_parser::MARKDOWN_OPTIONS;
use crate::document::writing::{heading_slug, safe_destination};
use crate::io::{assets, storage};
use storage::FileError;

pub fn html(source: &str, base: Option<&Path>) -> Result<String, FileError> {
    let mut events = Vec::new();
    let mut document_title = None;
    let mut embedded = 0;
    let mut cache = std::collections::HashMap::new();
    let mut heading_ids = std::collections::HashMap::<String, usize>::new();
    for (event, range) in Parser::new_ext(source, MARKDOWN_OPTIONS).into_offset_iter() {
        events.push(match event {
            Event::Start(Tag::Heading {
                level, id, classes, ..
            }) => {
                let label = Parser::new_ext(&source[range], MARKDOWN_OPTIONS)
                    .filter_map(|event| match event {
                        Event::Text(text) | Event::Code(text) => Some(text.into_string()),
                        _ => None,
                    })
                    .collect::<String>();
                if document_title.is_none() {
                    document_title = Some(label.clone());
                }
                let slug = id.map_or_else(|| heading_slug(&label), |id| id.into_string());
                let count = heading_ids.entry(slug.clone()).or_default();
                let id = if *count == 0 {
                    slug
                } else {
                    format!("{slug}-{count}")
                };
                *count += 1;
                Event::Start(Tag::Heading {
                    level,
                    id: Some(id.into()),
                    classes,
                    attrs: Vec::new(),
                })
            }

            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let destination = if let Some(path) = assets::local_path(&dest_url, base)? {
                    let data = if let Some(data) = cache.get(&path) {
                        data
                    } else {
                        let bytes = assets::read(&path).map_err(|e| {
                            FileError::Io(format!("Cannot export image {}: {e}", path.display()))
                        })?;
                        let format = assets::format(&bytes)?;
                        let mime = match format {
                            image::ImageFormat::Jpeg => "image/jpeg",
                            image::ImageFormat::Gif => "image/gif",
                            image::ImageFormat::WebP => "image/webp",
                            _ => "image/png",
                        };
                        let data = format!(
                            "data:{mime};base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(&bytes)
                        );
                        cache.entry(path).or_insert(data)
                    };
                    embedded += data.len();
                    if embedded > 128 * 1024 * 1024 {
                        return Err(FileError::Io(
                            "Export exceeds the 128 MiB embedded-image limit.".into(),
                        ));
                    }
                    data.clone()
                } else {
                    dest_url.into_string()
                };
                Event::Start(Tag::Image {
                    link_type,
                    dest_url: destination.into(),
                    title,
                    id,
                })
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let destination = if safe_destination(&dest_url) {
                    dest_url
                } else {
                    "#".into()
                };
                Event::Start(Tag::Link {
                    link_type,
                    dest_url: destination,
                    title,
                    id,
                })
            }
            other => other,
        });
    }
    Ok(storage::render_html(
        events.into_iter(),
        document_title.as_deref().unwrap_or("Dosty — document"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_upgrade_keeps_wikilinks_and_superscripts_literal_in_export() {
        let source = include_str!("../../tests/fixtures/parser-compatibility.md").trim_end();
        let exported = html(source, None).unwrap();
        assert!(exported.contains(&format!("<p>{source}</p>")));
        let legacy_strike = html("~deleted~", None).unwrap();
        assert!(legacy_strike.contains("<p><del>deleted</del></p>"));
    }

    #[test]
    fn exported_document_keeps_local_images_credits_and_safe_links_without_script_execution() {
        let dir = tempfile::tempdir().unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(3, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        std::fs::write(dir.path().join("a.png"), png.into_inner()).unwrap();
        let html = html("# A quiet hour\n\n![A coast](a.png)\n\n*A caption*\n\nPhoto: [Name](https://example.com)\n\n> Words\n>\n> — Author, [Source](#a-quiet-hour)\n\n[unsafe](javascript:alert)\n\n<script>bad()</script>", Some(dir.path())).unwrap();
        assert!(html.contains("src=\"data:image/png;base64,"));
        assert!(html.contains("id=\"a-quiet-hour\""));
        assert!(html.contains("<title>A quiet hour</title>"));
        assert!(html.contains("alt=\"A coast\""));
        assert!(html.contains("<em>A caption</em>"));
        assert!(html.contains("Photo:"));
        assert!(html.contains("— Author"));
        assert!(!html.contains("javascript:"));
        assert!(!html.contains("<script>"));
        assert!(super::html("![missing](missing.png)", Some(dir.path())).is_err());
    }
}
