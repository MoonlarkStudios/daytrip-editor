//! Reproducible, ignored large-document profiles using the real editor widget.
//! Software rasterization is included; OS delivery and GPU presentation are not.

use std::borrow::Cow;
use std::time::{Duration, Instant};

use iced::advanced::{
    Layout, Renderer as _, Shell, Widget, clipboard, input_method, layout, mouse, renderer, widget,
};
use iced::keyboard::{self, Key, key};
use iced::widget::text_editor::Binding;
use iced::{Event, Point, Rectangle, Renderer, Size};

use super::{App, Message};
use crate::document::selection::Selection;
use crate::editor::native_buffer;
use crate::editor::rich_editor::RichEditor;
use crate::theme::Preset;

fn fixture(blocks: usize) -> String {
    let mut source = String::new();
    for i in 0..blocks / 10 {
        source.push_str(&format!("## Chapter {i}\n\n"));
        for j in 0..4 {
            source.push_str(&format!("Paragraph {i}.{j}: **bold words**, *café 🦀*, `code` and [a useful link](https://example.com/a/long/destination). A few more words to wrap naturally at the editor width.\n\n"));
        }
        source.push_str("- First item\n- [x] Another **item**\n\n> A quotation with *emphasis*.\n\n```rust\nfn main() { println!(\"Hello\"); }\n```\n\n| Name | Value |\n|:---|---:|\n| **First** | 42 |\n\n![A local image](performance-missing.png)\n\n");
    }
    source
}

#[derive(Default, Clone, Copy)]
struct Sample {
    input: Duration,
    crdt: Duration,
    source: Duration,
    update: Duration,
    layout: Duration,
    draw: Duration,
    raster: Duration,
}
impl Sample {
    fn total(self) -> Duration {
        self.input + self.update + self.layout + self.draw + self.raster
    }
}

struct Harness {
    app: App,
    tree: widget::Tree,
    renderer: Renderer,
    pixels: tiny_skia::Pixmap,
    mask: tiny_skia::Mask,
    viewport: iced_tiny_skia::graphics::Viewport,
}

impl Harness {
    fn new(source: String) -> Self {
        let (mut app, task) = App::new();
        drop(task);
        app.resolved_theme = Preset::Paper.resolve();
        app.set_document(source, None, "Performance.md".into());
        app.editor_focused = true;
        app.sync_editor();
        let tree =
            widget::Tree::new(&Self::editor(&app) as &dyn Widget<Message, iced::Theme, Renderer>);
        Self {
            app,
            tree,
            renderer: Renderer::Secondary(iced_tiny_skia::Renderer::new(
                iced::Font::DEFAULT,
                18.0.into(),
            )),
            pixels: tiny_skia::Pixmap::new(720, 800).unwrap(),
            mask: tiny_skia::Mask::new(720, 800).unwrap(),
            viewport: iced_tiny_skia::graphics::Viewport::with_physical_size(
                Size::new(720, 800),
                1.0,
            ),
        }
    }
    fn editor(app: &App) -> RichEditor<'_> {
        RichEditor::new(
            app.editor.content(),
            &app.projection,
            app.document.source(),
            &app.resolved_theme,
            &app.images,
            Binding::from_key_press,
        )
        .collaboration(&app.document.collaboration)
        .preserve_viewport(
            app.preserve_viewport_generation == Some(app.document.collaboration.generation()),
        )
        .selection(app.selection)
        .focused(true)
    }
    fn frame(&mut self, sample: &mut Sample) {
        let size = Size::new(720.0, 800.0);
        let clip = Rectangle::with_size(size);
        let start = Instant::now();
        let mut editor = Self::editor(&self.app);
        let node = editor.layout(
            &mut self.tree,
            &self.renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        sample.layout += start.elapsed();
        let start = Instant::now();
        self.renderer.reset(clip);
        editor.draw(
            &self.tree,
            &mut self.renderer,
            &self.app.theme(),
            &renderer::Style {
                text_color: self.app.resolved_theme.tokens.text,
            },
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &clip,
        );
        sample.draw += start.elapsed();
        let start = Instant::now();
        if let Renderer::Secondary(renderer) = &mut self.renderer {
            renderer.draw(
                &mut self.pixels.as_mut(),
                &mut self.mask,
                &self.viewport,
                &[clip],
                self.app.resolved_theme.tokens.canvas,
            );
        }
        sample.raster += start.elapsed();
    }
    fn event(&mut self, event: Event, point: Point) -> Sample {
        let mut sample = Sample::default();
        let start = Instant::now();
        let mut messages = Vec::new();
        let mut editor = Self::editor(&self.app);
        let size = Size::new(720.0, 800.0);
        let node = editor.layout(
            &mut self.tree,
            &self.renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        editor.update(
            &mut self.tree,
            &event,
            Layout::new(&node),
            mouse::Cursor::Available(point),
            &self.renderer,
            &mut clipboard::Null,
            &mut Shell::new(&mut messages),
            &Rectangle::with_size(size),
        );
        sample.input = start.elapsed();
        let start = Instant::now();
        for message in messages {
            drop(self.app.update(message));
            sample.crdt += self.app.document.costs.crdt;
            sample.source += self.app.document.costs.source;
        }
        sample.update = start.elapsed();
        self.frame(&mut sample);
        sample
    }
    fn message(&mut self, message: Message) -> Sample {
        let start = Instant::now();
        drop(self.app.update(message));
        let mut sample = Sample {
            update: start.elapsed(),
            crdt: self.app.document.costs.crdt,
            source: self.app.document.costs.source,
            ..Sample::default()
        };
        self.frame(&mut sample);
        sample
    }
}

fn report(name: &str, samples: &[Sample]) {
    type Phase = (&'static str, fn(Sample) -> Duration);
    let fields: [Phase; 9] = [
        ("input", |s| s.input),
        ("update", |s| s.update),
        ("crdt", |s| s.crdt),
        ("source", |s| s.source),
        ("adapters", |s| s.update.saturating_sub(s.crdt + s.source)),
        ("layout", |s| s.layout),
        ("draw", |s| s.draw),
        ("raster", |s| s.raster),
        ("total", Sample::total),
    ];
    print!("{name} n={}", samples.len());
    for (name, field) in fields {
        let mut values = samples
            .iter()
            .map(|s| field(*s).as_secs_f64() * 1000.0)
            .collect::<Vec<_>>();
        values.sort_by(f64::total_cmp);
        print!(
            " {name}={:.3}/{:.3}/{:.3}ms",
            values[values.len() / 2],
            values[(values.len() * 95).div_ceil(100).saturating_sub(1)],
            values[values.len() - 1]
        );
    }
    println!(" (p50/p95/max)");
}

fn typing_event() -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Character("x".into()),
        modified_key: Key::Character("x".into()),
        physical_key: key::Physical::Code(key::Code::KeyX),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: Some("x".into()),
        repeat: false,
    })
}

/// Current test-process RSS, distinct from the command's process-tree peak.
fn report_memory(label: &str) {
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        && output.status.success()
    {
        println!(
            "memory/{label} rss_kib={}",
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }
    #[cfg(not(target_os = "macos"))]
    let _ = label;
}

fn load_profile_fonts() {
    let mut fonts = iced_tiny_skia::graphics::text::font_system()
        .write()
        .unwrap();
    for font in [
        include_bytes!("../../assets/fonts/Newsreader-Regular.ttf").as_slice(),
        include_bytes!("../../assets/fonts/Newsreader-Bold.ttf").as_slice(),
        include_bytes!("../../assets/fonts/Newsreader-Italic.ttf").as_slice(),
        include_bytes!("../../assets/fonts/Newsreader-BoldItalic.ttf").as_slice(),
    ] {
        fonts.load_font(Cow::Borrowed(font));
    }
    drop(fonts);
}

#[test]
#[ignore = "render code surfaces for visual inspection without opening user documents"]
fn code_surface_previews() {
    load_profile_fonts();
    for preset in Preset::ALL {
        let source = include_str!("../../tests/fixtures/code-editing.md");
        let mut harness = Harness::new(source.into());
        harness.app.resolved_theme = preset.resolve();
        let at = source.find("let message").unwrap() + 4;
        harness.message(Message::SelectSource(Selection::caret(at)));
        image::save_buffer(
            format!("/tmp/daytrip-code-{}.png", preset.name()),
            harness.pixels.data(),
            harness.pixels.width(),
            harness.pixels.height(),
            image::ColorType::Rgba8,
        )
        .unwrap();
        let items = iced::widget::markdown::parse(source).collect::<Vec<_>>();
        let viewer = crate::editor::markdown_view::MarkdownView {
            tokens: harness.app.resolved_theme.tokens,
            block: 0,
            base: None,
            line_height: harness.app.resolved_theme.typography.line_height,
            task: std::cell::Cell::new(0),
        };
        let mut reader = iced::widget::markdown::view_with(
            &items,
            crate::theme::styles::markdown_settings(&harness.app.resolved_theme),
            &viewer,
        );
        let mut tree = widget::Tree::new(reader.as_widget());
        let size = Size::new(720.0, 800.0);
        let clip = Rectangle::with_size(size);
        let node = reader.as_widget_mut().layout(
            &mut tree,
            &harness.renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        harness.renderer.reset(clip);
        reader.as_widget().draw(
            &tree,
            &mut harness.renderer,
            &harness.app.theme(),
            &renderer::Style {
                text_color: viewer.tokens.text,
            },
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &clip,
        );
        if let Renderer::Secondary(renderer) = &mut harness.renderer {
            renderer.draw(
                &mut harness.pixels.as_mut(),
                &mut harness.mask,
                &harness.viewport,
                &[clip],
                viewer.tokens.canvas,
            );
        }
        image::save_buffer(
            format!("/tmp/daytrip-code-read-{}.png", preset.name()),
            harness.pixels.data(),
            harness.pixels.width(),
            harness.pixels.height(),
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}

#[test]
#[ignore = "run with mise run bench; long code block editing through software rasterization"]
fn performance_code_editor() {
    load_profile_fonts();
    let code = (0..2000)
        .map(|i| format!("    let item_{i} = \"value 🦀\";\n"))
        .collect::<String>();
    let mut harness = Harness::new(format!(
        "Before\n\n```rust\nfn main() {{\n{code}}}\n```\n\nAfter"
    ));
    let at = harness.app.document.source().find("item_1000").unwrap() + 5;
    harness.message(Message::SelectSource(Selection::caret(at)));
    harness.event(
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Point::new(100.0, 300.0),
    );
    harness.message(Message::SelectSource(Selection::caret(at)));
    let mut samples = Vec::new();
    for _ in 0..20 {
        samples.push(harness.event(typing_event(), Point::new(100.0, 300.0)));
    }
    assert!(
        harness
            .app
            .document
            .source()
            .contains(&format!("item_{}1000", "x".repeat(20)))
    );
    report("code/typing-middle-2000-lines", &samples);
    println!(
        "code/layout_counts={:?}",
        RichEditor::profile_stats(&harness.tree)
    );
}

#[test]
#[ignore = "run with mise run profile; large documents and software rasterization"]
fn large_document_interactions() {
    load_profile_fonts();
    let counts = std::env::var("DAYTRIP_PROFILE_BLOCKS").unwrap_or_else(|_| "10000,50000".into());
    let samples = std::env::var("DAYTRIP_PROFILE_SAMPLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30_usize)
        .max(1);
    for blocks in counts.split(',').map(|s| s.parse::<usize>().unwrap()) {
        let source = fixture(blocks);
        let bytes = source.len();
        let start = Instant::now();
        let mut harness = Harness::new(source);
        let load = start.elapsed();
        assert_eq!(harness.app.document.blocks.len(), blocks);
        report_memory("source-load");
        let mut cold = Sample::default();
        harness.frame(&mut cold);
        println!(
            "\nblocks={blocks} bytes={bytes} load={load:?} layout_counts={:?}",
            RichEditor::profile_stats(&harness.tree)
        );
        report("cold", &[cold]);
        report_memory("cold");
        if std::env::var_os("DAYTRIP_PROFILE_COLD_ONLY").is_some() {
            continue;
        }
        let mut jumps = Vec::new();
        for step in [9, 0, 5, 2, 8, 1, 7, 3, 6, 4] {
            let offset = harness.app.document.blocks[step * blocks / 10].range.start;
            jumps.push(harness.message(Message::SelectSource(Selection::caret(offset))));
        }
        report("jump/distant", &jumps);
        report_memory("distant-jumps");
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            harness.event(Event::Mouse(event), Point::new(200.0, 200.0));
        }
        for (name, fraction) in [("start", 0), ("middle", 1), ("end", 2)] {
            let offset = if fraction == 2 {
                harness.app.document.source().len()
            } else {
                let index = fraction * blocks / 2 + 1;
                harness.app.document.blocks[index].range.start + 12
            };
            harness.message(Message::SelectSource(Selection::caret(offset)));
            let before = harness.app.document.source().len();
            let timings = (0..samples)
                .map(|_| harness.event(typing_event(), Point::new(200.0, 200.0)))
                .collect::<Vec<_>>();
            assert_eq!(harness.app.document.source().len(), before + samples);
            report(&format!("typing/{name}"), &timings);
        }
        let middle = harness.app.document.blocks[blocks / 2 + 1].range.start;
        let bold = middle
            + harness.app.document.source()[middle..]
                .find("bold words")
                .unwrap();
        harness.message(Message::SelectSource(Selection::caret(middle + 2)));
        let timings = (0..samples)
            .map(|i| {
                harness.message(Message::SelectSource(Selection::caret(if i % 2 == 0 {
                    bold + 2
                } else {
                    middle + 2
                })))
            })
            .collect::<Vec<_>>();
        report("disclosure", &timings);
        let anchor = harness.app.document.blocks[blocks / 2].range.start;
        let timings = (0..samples)
            .map(|i| {
                harness.message(Message::SelectSource(Selection {
                    anchor,
                    head: middle + 4 + i % 6,
                }))
            })
            .collect::<Vec<_>>();
        report("selection/local", &timings);
        let timings = (0..samples)
            .map(|i| {
                harness.message(Message::SelectSource(Selection {
                    anchor: if i % 2 == 0 {
                        0
                    } else {
                        harness.app.document.source().len()
                    },
                    head: middle + 4,
                }))
            })
            .collect::<Vec<_>>();
        report("selection/half-document", &timings);
        let timings = (0..samples)
            .map(|_| {
                harness.event(
                    Event::Mouse(mouse::Event::WheelScrolled {
                        delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -120.0 },
                    }),
                    Point::new(200.0, 300.0),
                )
            })
            .collect::<Vec<_>>();
        report("scroll", &timings);
        harness.message(Message::SelectSource(Selection::caret(middle + 5)));
        let timings = (0..samples)
            .map(|_| {
                harness.event(
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: Key::Named(key::Named::ArrowDown),
                        modified_key: Key::Named(key::Named::ArrowDown),
                        physical_key: key::Physical::Code(key::Code::ArrowDown),
                        location: keyboard::Location::Standard,
                        modifiers: keyboard::Modifiers::SHIFT,
                        text: None,
                        repeat: false,
                    }),
                    Point::new(200.0, 200.0),
                )
            })
            .collect::<Vec<_>>();
        report("selection/keyboard", &timings);
        harness.event(
            Event::Keyboard(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::empty(),
            )),
            Point::ORIGIN,
        );

        harness.event(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Point::new(160.0, 100.0),
        );
        let timings = (0..samples)
            .map(|i| {
                let point = Point::new(
                    160.0 + (i % 8) as f32 * 30.0,
                    200.0 + (i % 12) as f32 * 40.0,
                );
                harness.event(
                    Event::Mouse(mouse::Event::CursorMoved { position: point }),
                    point,
                )
            })
            .collect::<Vec<_>>();
        report("selection/drag", &timings);
        harness.event(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Point::new(200.0, 300.0),
        );
        harness.message(Message::SelectSource(Selection::caret(middle + 3)));
        let timings = (0..samples)
            .map(|_| {
                harness.message(Message::Edit(iced::widget::text_editor::Action::Edit(
                    iced::widget::text_editor::Edit::Insert('_'),
                )))
            })
            .collect::<Vec<_>>();
        report("typing/syntax-fallback", &timings);

        // Composition stays transient until commit, even deep in a large
        // document. Undo must restore the source and both selection endpoints.
        harness.message(Message::SelectSource(Selection::caret(middle + 12)));
        let before = harness.app.document.source().to_owned();
        let selection = harness.app.selection;
        harness.event(
            Event::InputMethod(input_method::Event::Opened),
            Point::ORIGIN,
        );
        let preedit = harness.event(
            Event::InputMethod(input_method::Event::Preedit("にほん".into(), Some(0..9))),
            Point::ORIGIN,
        );
        assert_eq!(harness.app.document.source(), before);
        assert_eq!(harness.app.selection, selection);
        report("ime/preedit", &[preedit]);
        let committed = "日本語 e\u{301} 🧑‍💻";
        let commit = harness.event(
            Event::InputMethod(input_method::Event::Commit(committed.into())),
            Point::ORIGIN,
        );
        let mut after = before.clone();
        after.insert_str(selection.head, committed);
        assert_eq!(harness.app.document.source(), after);
        report("ime/commit", &[commit]);
        let undo = harness.message(Message::Undo);
        assert_eq!(harness.app.document.source(), before);
        assert_eq!(harness.app.selection, selection);
        report("undo/ime", &[undo]);
        let redo = harness.message(Message::Redo);
        assert_eq!(harness.app.document.source(), after);
        report("redo/ime", &[redo]);
        println!(
            "final_layout_counts={:?}",
            RichEditor::profile_stats(&harness.tree)
        );
        report_memory("final");
        // Complete reads are correctness oracles outside the timed event path.
        assert_eq!(
            harness.app.editor.snapshot(),
            harness.app.editor.content().text()
        );
        assert_eq!(harness.app.editor.snapshot(), harness.app.projection.text);
    }
}

#[test]
#[ignore = "component diagnostics for large-document load"]
fn large_document_load_components() {
    let source = fixture(50_000);
    let start = Instant::now();
    let document = crate::document::Document::new(source.clone());
    println!("load/document {:?}", start.elapsed());
    let start = Instant::now();
    let projection = crate::document::projection::Projection::new(&source);
    println!("load/projection {:?}", start.elapsed());
    let start = Instant::now();
    let mut content = native_buffer::Buffer::new(&projection.text);
    println!("load/native-init {:?}", start.elapsed());
    let start = Instant::now();
    content.select(Selection::caret(0));
    println!("load/native-caret {:?}", start.elapsed());
    assert_eq!(document.blocks.len(), 50_000);
    std::hint::black_box(content);
}

#[test]
#[ignore = "run using mise run bench; source parsing without native/render costs"]
fn performance_incremental_block_parsing() {
    for count in [10_000, 50_000] {
        let source = fixture(count);
        let mut document = crate::document::Document::new(source);
        let mut parsed = 0;
        let mut full_equivalent = 0;
        let start = Instant::now();
        for _ in 0..100 {
            let point = document.blocks[count / 2 + 1].range.start + 12;
            document.replace(point..point, "日_").unwrap();
            let change = document.block_changes.first().expect("isolated paragraph");
            assert_eq!(change.previous_revision + 1, document.revision);
            parsed += change.new.len();
            full_equivalent += document.source().len();
        }
        println!(
            "blocks={count} local_source_edits=100 elapsed={:?} boundary_scan_bytes={parsed} full_scan_equivalent_bytes={full_equivalent}",
            start.elapsed()
        );
        assert_eq!(document.blocks.len(), count);
        let full = crate::document::Document::new(document.source().into());
        assert_eq!(
            (document.words, document.characters),
            (full.words, full.characters)
        );
        assert!(
            document
                .blocks
                .iter()
                .zip(&full.blocks)
                .all(|(a, b)| a.range == b.range && a.heading == b.heading)
        );
    }
}

#[test]
#[ignore = "run using mise run bench; source/projection phases without native/render costs"]
fn performance_source_projection_updates() {
    use crate::document::projection::Projection;

    for blocks in [10_000, 50_000] {
        for kind in ["word", "unicode-syntax", "equal-length"] {
            let mut document = crate::document::Document::new(fixture(blocks));
            let mut head = document.blocks[blocks / 2 + 1].range.start + 11;
            let mut projection = Projection::with_context(document.source(), Some(head));
            let payloads = projection
                .rich
                .images
                .iter()
                .map(|image| image.url.as_ptr())
                .collect::<Vec<_>>();
            let mut source_time = Duration::ZERO;
            let mut projection_time = Duration::ZERO;
            let mut mirror_bytes = 0;
            let mut full_equivalent = 0;
            for step in 0..100 {
                let (removed, inserted) = match kind {
                    "word" => (0, "x"),
                    "unicode-syntax" => (0, "日_"),
                    _ => (1, if step % 2 == 0 { "q" } else { "r" }),
                };
                let start = Instant::now();
                document.replace(head..head + removed, inserted).unwrap();
                source_time += start.elapsed();
                if removed == 0 {
                    head += inserted.len();
                }
                let start = Instant::now();
                let word = projection.update_words(document.source(), Some(head));
                if !word {
                    assert!(projection.update_blocks(
                        document.source(),
                        document.block_changes.first().expect("isolated paragraph"),
                        Some(head)
                    ));
                }
                projection_time += start.elapsed();
                mirror_bytes += if word {
                    inserted.len()
                } else {
                    document.block_changes.first().unwrap().new.len()
                };
                full_equivalent += document.source().len();
            }
            println!(
                "blocks={blocks} kind={kind} edits=100 source_including_crdt={source_time:?} crdt={:?} source_cache={:?} projection={projection_time:?} source_mirror_splice_bytes={mirror_bytes} full_source_equivalent_bytes={full_equivalent}",
                document.costs.crdt, document.costs.source
            );
            assert_eq!(projection.source, document.source());
            let full = Projection::with_context(document.source(), Some(head));
            assert_eq!(projection.text, full.text);
            assert_eq!(projection.highlights, full.highlights);
            assert_eq!(projection.rich, full.rich);
            assert_eq!(
                payloads,
                projection
                    .rich
                    .images
                    .iter()
                    .map(|image| image.url.as_ptr())
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn crdt_incoming_changes_preserve_composition_selection_and_derived_editor_state() {
    use crate::document::Document;
    use iced::widget::text_editor::{Action, Edit};
    let source = "# 日本語\r\n\r\n| A | B |\n|---|---|\n| first | last |\n\n![alt](image.png)\n\nFinal paragraph.";
    let mut harness = Harness::new(source.into());
    harness.frame(&mut Sample::default());
    harness.event(
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Point::new(80.0, 40.0),
    );
    let mut remote = Document::from_state(&harness.app.document.collaboration.state()).unwrap();
    let anchor = source.find("Final").unwrap();
    let head = source.find("first").unwrap();
    harness.message(Message::SelectSource(Selection { anchor, head }));
    remote.replace(0..0, "Prefix\n\n").unwrap();
    harness.message(Message::ReplicaLoaded(Ok(remote.collaboration.state())));
    assert_eq!(
        harness.app.selection,
        Selection {
            anchor: anchor + 8,
            head: head + 8
        }
    );
    assert_eq!(harness.app.editor.snapshot(), harness.app.projection.text);
    // Use a caret while composing; concurrent insertion before it rebases target.
    harness.message(Message::SelectSource(Selection::caret(head + 8)));
    let unchanged = harness.app.document.source().to_owned();
    harness.event(
        Event::InputMethod(input_method::Event::Opened),
        Point::ORIGIN,
    );
    harness.event(
        Event::InputMethod(input_method::Event::Preedit("にほん".into(), Some(0..9))),
        Point::ORIGIN,
    );
    assert_eq!(harness.app.document.source(), unchanged);
    assert!(RichEditor::intent_stats(&harness.tree).1);
    harness.event(
        Event::InputMethod(input_method::Event::Preedit(String::new(), None)),
        Point::ORIGIN,
    );
    remote.replace(0..0, "λ").unwrap();
    harness.message(Message::ReplicaLoaded(Ok(remote.collaboration.state())));
    assert!(RichEditor::intent_stats(&harness.tree).1);
    harness.event(
        Event::InputMethod(input_method::Event::Commit("日本語 🧑‍💻".into())),
        Point::ORIGIN,
    );
    assert_eq!(
        harness.app.document.source().matches("日本語 🧑‍💻").count(),
        1
    );
    assert!(harness.app.document.source().contains("日本語 🧑‍💻first"));
    assert_eq!(harness.app.editor.snapshot(), harness.app.projection.text);
    harness.message(Message::Undo);
    assert_eq!(harness.app.document.source(), remote.source());
    harness.message(Message::Redo);
    assert_eq!(
        harness.app.document.source().matches("日本語 🧑‍💻").count(),
        1
    );
    harness.message(Message::Edit(Action::Edit(Edit::Insert('!'))));
    assert_eq!(
        harness.app.document.source(),
        harness.app.document.collaboration.materialize()
    );
}

#[test]
fn crdt_composition_replacement_preserves_remote_content_when_target_is_deleted() {
    use crate::document::Document;
    let mut harness = Harness::new("Before **selected** after".into());
    harness.frame(&mut Sample::default());
    harness.event(
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Point::new(80.0, 40.0),
    );
    let mut remote = Document::from_state(&harness.app.document.collaboration.state()).unwrap();
    harness.message(Message::SelectSource(Selection {
        anchor: 9,
        head: 17,
    }));
    harness.event(
        Event::InputMethod(input_method::Event::Opened),
        Point::ORIGIN,
    );
    harness.event(
        Event::InputMethod(input_method::Event::Preedit("に".into(), None)),
        Point::ORIGIN,
    );
    remote.replace(9..17, "remote").unwrap();
    harness.message(Message::ReplicaLoaded(Ok(remote.collaboration.state())));
    harness.event(
        Event::InputMethod(input_method::Event::Commit("日本".into())),
        Point::ORIGIN,
    );
    assert!(harness.app.document.source().contains("remote"));
    assert_eq!(harness.app.document.source().matches("日本").count(), 1);
    assert_eq!(
        harness.app.document.source(),
        harness.app.document.collaboration.materialize()
    );
}

#[test]
fn crdt_proposal_uses_real_app_transaction_projection_and_independent_agent_undo() {
    use crate::document::collaboration::{AgentProposal, ProposedEdit};
    let mut harness = Harness::new("# Title\n\nFirst paragraph.\n\nLast paragraph.".into());
    harness.frame(&mut Sample::default());
    let base = harness
        .app
        .document
        .collaboration
        .snapshot(harness.app.document.source());
    let first = base.source.find("First").unwrap();
    let last = base.source.find("Last").unwrap();
    let proposal = AgentProposal {
        base,
        author: "Writer".into(),
        edits: vec![
            ProposedEdit {
                start: first,
                end: first + 5,
                expected: "First".into(),
                replacement: "Better".into(),
            },
            ProposedEdit {
                start: last,
                end: last + 4,
                expected: "Last".into(),
                replacement: "Final".into(),
            },
        ],
    };
    harness.message(Message::ProposalLoaded(Ok(proposal.clone())));
    assert!(harness.app.document.source().contains("Better paragraph"));
    assert!(harness.app.document.source().contains("Final paragraph"));
    assert_eq!(harness.app.editor.snapshot(), harness.app.projection.text);
    assert!(!harness.app.document.can_undo());
    harness.message(Message::AgentUndo);
    assert_eq!(harness.app.document.source(), proposal.base.source);
    harness.message(Message::AgentRedo);
    assert!(harness.app.document.source().contains("Final paragraph"));
    let before = harness.app.document.source().to_owned();
    harness.message(Message::ProposalLoaded(Ok(proposal)));
    assert_eq!(harness.app.document.source(), before);
    assert!(
        harness
            .app
            .notification
            .as_ref()
            .unwrap()
            .contains("conflict")
    );
}

#[test]
fn crdt_drag_and_viewport_anchors_survive_remote_edits_before_the_viewport() {
    use crate::document::Document;
    let mut harness = Harness::new(fixture(1000));
    harness.frame(&mut Sample::default());
    harness.event(
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Point::new(110.0, 100.0),
    );
    let drag = RichEditor::intent_stats(&harness.tree).2.unwrap();
    let mut remote = Document::from_state(&harness.app.document.collaboration.state()).unwrap();
    remote.replace(0..0, "Prefix\n\n").unwrap();
    harness.message(Message::ReplicaLoaded(Ok(remote.collaboration.state())));
    assert_eq!(RichEditor::intent_stats(&harness.tree).2, Some(drag + 8));
    harness.event(
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        Point::new(110.0, 100.0),
    );
    harness.message(Message::SelectSource(Selection::caret(0)));
    harness.event(
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -1200.0 },
        }),
        Point::new(200.0, 200.0),
    );
    // The caret remains offscreen while the first visible row anchors reflow.
    let visible = RichEditor::visible_anchor(&harness.tree).unwrap();
    let y = RichEditor::screen_y(&harness.tree, visible);
    let old_scroll = RichEditor::intent_stats(&harness.tree).0;
    remote.replace(0..0, "Another prefix\n\n").unwrap();
    harness.message(Message::ReplicaLoaded(Ok(remote.collaboration.state())));
    let new_y = RichEditor::screen_y(&harness.tree, visible + 16);
    assert!(
        (y - new_y).abs() < 1.0,
        "screen anchor moved: {y} -> {new_y}, scroll {old_scroll} -> {}",
        RichEditor::intent_stats(&harness.tree).0
    );
    assert_eq!(RichEditor::intent_stats(&harness.tree).2, None);
}

#[test]
#[ignore = "run with mise run bench; CRDT retention, memory, and persistent growth"]
fn performance_crdt_churn() {
    use crate::document::Document;
    for blocks in [10_000, 50_000] {
        let source = fixture(blocks);
        let start = Instant::now();
        let mut document = Document::new(source.clone());
        let cold = start.elapsed();
        let initial = document.collaboration.state().update.len();
        report_memory(&format!("crdt/{blocks}/initial"));
        let start = Instant::now();
        document.costs = Default::default();
        for iteration in 0..1000 {
            let chapter = iteration * 997 % (blocks / 10);
            let point = document.blocks[chapter * 10 + 1].range.start + 12;
            document.break_history_group();
            document
                .replace(point..point, "Churn é 🧑‍💻 material 0123456789")
                .unwrap();
            document
                .replace(point..point + "Churn é 🧑‍💻 material 0123456789".len(), "")
                .unwrap();
        }
        let elapsed = start.elapsed();
        report_memory(&format!("crdt/{blocks}/churn"));
        let state = document.collaboration.state();
        println!(
            "crdt/churn blocks={blocks} transactions=2000 locations=1000 cold={cold:?} total={elapsed:?} crdt={:?} source_cache={:?} initial_state_bytes={initial} churn_state_bytes={} source_bytes={}",
            document.costs.crdt,
            document.costs.source,
            state.update.len(),
            document.source().len()
        );
        assert_eq!(document.source(), source);
        assert_eq!(document.collaboration.materialize(), source);
        let start = Instant::now();
        let reopened = Document::from_state(&state).unwrap();
        let reopen = start.elapsed();
        println!(
            "crdt/reopen blocks={blocks} total={reopen:?} checkpoint_bytes={}",
            crate::io::collaboration_store::encode(&state)
                .unwrap()
                .len()
        );
        assert_eq!(reopened.source(), source);
    }
}

#[test]
#[ignore = "offline remote batches through the real application, layout, and rasterizer"]
fn large_document_collaboration() {
    load_profile_fonts();
    use crate::document::collaboration::{Collaboration, EditOrigin, SourceEdit};
    let samples = std::env::var("DAYTRIP_PROFILE_SAMPLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30_usize)
        .max(1);
    for blocks in [10_000, 50_000] {
        let mut harness = Harness::new(fixture(blocks));
        harness.frame(&mut Sample::default());
        let (mut remote, mut source) =
            Collaboration::from_state(&harness.app.document.collaboration.state()).unwrap();
        let mut timings = Vec::new();
        for _ in 0..samples {
            let edits: Vec<_> = (1..=8)
                .map(|step| {
                    let point = harness.app.document.blocks[step * blocks / 10 + 1]
                        .range
                        .start
                        + 12;
                    SourceEdit {
                        range: point..point,
                        text: "日".into(),
                    }
                })
                .collect();
            let committed = remote
                .commit(&source, &edits, EditOrigin::Human, None, true)
                .unwrap();
            for edit in committed.into_iter().rev() {
                source.replace_range(edit.range, &edit.text);
            }
            let update = remote.state(); // explicit offline export outside the interaction timer
            timings.push(harness.message(Message::ReplicaLoaded(Ok(update))));
            assert_eq!(harness.app.document.source(), source);
        }
        report(&format!("remote/batch-8/{blocks}"), &timings);
        report_memory(&format!("remote/{blocks}/final"));
        let before = harness.app.document.source().to_owned();
        let initial_state = harness.app.document.collaboration.state().update.len() + 64;
        let point = harness.app.document.blocks[blocks / 2 + 1].range.start + 12;
        harness.event(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Point::new(80.0, 40.0),
        );
        harness.event(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Point::new(80.0, 40.0),
        );
        harness.message(Message::SelectSource(Selection::caret(point)));
        let mut churn = Vec::new();
        for _ in 0..200 {
            churn.push(
                harness.message(Message::Edit(iced::widget::text_editor::Action::Edit(
                    iced::widget::text_editor::Edit::Paste(std::sync::Arc::new(
                        "churn é 🧑‍💻".into(),
                    )),
                ))),
            );
            churn.push(harness.message(Message::Undo));
        }
        report(&format!("app/churn-400/{blocks}"), &churn);
        report_memory(&format!("app/{blocks}/after-churn"));
        println!(
            "app/persistence blocks={blocks} before_churn_bytes={initial_state} after_churn_bytes={}",
            harness.app.document.collaboration.state().update.len() + 64
        );
        assert_eq!(harness.app.document.source(), before);

        assert_eq!(
            harness.app.document.source(),
            harness.app.document.collaboration.materialize()
        );
        assert_eq!(harness.app.editor.snapshot(), harness.app.projection.text);
    }
}

#[test]
fn crdt_duplicate_update_keeps_saved_generation_and_projection_oracle() {
    use crate::document::projection::Projection;
    let mut harness = Harness::new("# Title\n\n**bold** and 🧑‍💻\n\n![alt](a.png)".into());
    let state = harness.app.document.collaboration.state();
    harness.message(Message::ReplicaLoaded(Ok(state.clone())));
    harness.message(Message::ReplicaLoaded(Ok(state)));
    assert!(!harness.app.dirty);
    let fresh = Projection::with_context(
        harness.app.document.source(),
        Some(harness.app.selection.head),
    );
    assert_eq!(fresh.text, harness.app.projection.text);
    for (offset, _) in harness.app.document.source().char_indices() {
        assert_eq!(
            fresh.to_display(offset),
            harness.app.projection.to_display(offset)
        );
    }
    assert_eq!(harness.app.editor.snapshot(), fresh.text);
}

#[test]
fn crdt_reopening_the_same_identity_clears_old_composition_session() {
    use crate::document::Document;
    let mut harness = Harness::new("# Title\n\ntext".into());
    harness.frame(&mut Sample::default());
    harness.event(
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Point::new(80.0, 40.0),
    );
    harness.event(
        Event::InputMethod(input_method::Event::Opened),
        Point::ORIGIN,
    );
    let target = harness
        .app
        .document
        .collaboration
        .composition_target(harness.app.selection);
    assert!(RichEditor::intent_stats(&harness.tree).1);
    let document = Document::from_state(&harness.app.document.collaboration.state()).unwrap();
    harness
        .app
        .install_document(document, None, "Reopened.md".into());
    harness.frame(&mut Sample::default());
    assert!(!RichEditor::intent_stats(&harness.tree).1);
    let before = harness.app.document.source().to_owned();
    harness.message(Message::CommitComposition {
        target,
        text: "stale".into(),
    });
    assert_eq!(harness.app.document.source(), before);
}

#[test]
fn crdt_replace_all_rebases_reverse_selection_through_earlier_changes() {
    let source =
        "one one\n\n# Title\n\n| A | B |\n|---|---|\n| left | right |\n\n![alt](a.png)\n\nTail";
    let mut harness = Harness::new(source.into());
    let head = source.find("Title").unwrap();
    harness.message(Message::SelectSource(Selection {
        anchor: source.len(),
        head,
    }));
    harness.app.query = "one".into();
    harness.app.replacement = "expanded".into();
    harness.message(Message::ReplaceAll);
    assert_eq!(
        harness.app.selection,
        Selection {
            anchor: source.len() + 10,
            head: head + 10
        }
    );
    assert_eq!(harness.app.editor.snapshot(), harness.app.projection.text);
    assert_eq!(
        harness.app.document.source(),
        harness.app.document.collaboration.materialize()
    );
}

#[test]
fn canceled_preedit_recaptures_the_next_composition_target_in_both_editors() {
    fn event(app: &mut App, tree: &mut widget::Tree, renderer: &Renderer, event: Event) {
        let size = Size::new(720.0, 800.0);
        let mut messages = Vec::new();
        {
            let mut editor = app.editor_view(app.mode == super::EditorMode::Source);
            let node = editor.as_widget_mut().layout(
                tree,
                renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            editor.as_widget_mut().update(
                tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(Point::new(70.0, 20.0)),
                renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
        }
        for message in messages {
            drop(app.update(message));
        }
    }
    let original = "# Heading\r\n\r\nBefore selected after\n\n| A | B |\n|---|---|\n| one | two |\n\n![alt](missing.png)\n\nFinal words.";
    for source_mode in [false, true] {
        for (preedit, committed) in [("にほん", "日本語"), ("zhongwen", "中文"), ("ㅎ", "한국어")]
        {
            let (mut app, task) = App::new();
            drop(task);
            app.set_document(original.into(), None, "Composition.md".into());
            if source_mode {
                drop(app.update(Message::ToggleSource));
            }
            let mut tree = widget::Tree::new(app.editor_view(source_mode).as_widget());
            let renderer = Renderer::Secondary(iced_tiny_skia::Renderer::new(
                iced::Font::DEFAULT,
                18.0.into(),
            ));
            let mut send = |app: &mut App, value| event(app, &mut tree, &renderer, value);
            for button in [
                mouse::Event::ButtonPressed(mouse::Button::Left),
                mouse::Event::ButtonReleased(mouse::Button::Left),
            ] {
                send(&mut app, Event::Mouse(button));
            }
            // Seed the same reverse cross-block native selection in both modes.
            let canceled_selection = Selection {
                anchor: original.find("Final").unwrap(),
                head: original.find("Heading").unwrap(),
            };
            app.selection = canceled_selection;
            app.sync_editor();
            for input in [
                input_method::Event::Opened,
                input_method::Event::Preedit("cancel".into(), Some(0..6)),
                input_method::Event::Preedit(String::new(), None),
            ] {
                send(&mut app, Event::InputMethod(input));
            }
            assert_eq!(app.document.source(), original);
            assert!(!app.document.can_undo());
            assert_eq!(app.selection, canceled_selection);
            let target = original.find("words").unwrap();
            app.selection = Selection {
                anchor: target + 5,
                head: target,
            };
            app.sync_editor();
            send(
                &mut app,
                Event::InputMethod(input_method::Event::Preedit(preedit.into(), None)),
            );
            assert_eq!(app.document.source(), original);
            // macOS clears marked text immediately before delivering a commit.
            send(
                &mut app,
                Event::InputMethod(input_method::Event::Preedit(String::new(), None)),
            );
            send(
                &mut app,
                Event::InputMethod(input_method::Event::Commit(committed.into())),
            );
            let expected = original.replacen("words", committed, 1);
            assert_eq!(app.document.source(), expected, "source_mode={source_mode}");
            assert_eq!(app.document.collaboration.materialize(), expected);
            assert_eq!(app.selection, Selection::caret(target + committed.len()));
            assert_eq!(
                app.editor.snapshot(),
                if source_mode {
                    app.document.source()
                } else {
                    &app.projection.text
                }
            );
            drop(app.update(Message::Undo));
            assert_eq!(app.document.source(), original);
            drop(app.update(Message::Redo));
            assert_eq!(app.document.source(), expected);
            // A direct commit after cancellation and a local motion must use
            // the new caret, even when the next IME emits no nonempty preedit.
            for input in [
                input_method::Event::Preedit("cancel".into(), None),
                input_method::Event::Preedit(String::new(), None),
            ] {
                send(&mut app, Event::InputMethod(input));
            }
            send(
                &mut app,
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: Key::Named(key::Named::ArrowLeft),
                    modified_key: Key::Named(key::Named::ArrowLeft),
                    physical_key: key::Physical::Code(key::Code::ArrowLeft),
                    location: keyboard::Location::Standard,
                    modifiers: keyboard::Modifiers::empty(),
                    text: None,
                    repeat: false,
                }),
            );
            let caret = app.selection.head;
            let mut next = expected.clone();
            next.insert(caret, '好');
            send(
                &mut app,
                Event::InputMethod(input_method::Event::Commit("好".into())),
            );
            assert_eq!(app.document.source(), next);
            drop(app.update(Message::Undo));
            assert_eq!(app.document.source(), expected);
        }
    }
}

#[test]
fn crdt_source_widget_composition_uses_the_same_stable_exactly_once_commit() {
    use crate::document::Document;
    fn event(app: &mut App, tree: &mut widget::Tree, renderer: &Renderer, event: Event) {
        let size = Size::new(720.0, 800.0);
        let mut messages = Vec::new();
        {
            let mut editor = app.editor_view(true);
            let node = editor.as_widget_mut().layout(
                tree,
                renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            editor.as_widget_mut().update(
                tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(Point::new(70.0, 20.0)),
                renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
        }
        for message in messages {
            drop(app.update(message));
        }
    }
    let (mut app, task) = App::new();
    drop(task);
    app.set_document(
        "Before selected after".into(),
        None,
        "Composition.md".into(),
    );
    drop(app.update(Message::ToggleSource));
    let mut tree = widget::Tree::new(app.editor_view(true).as_widget());
    let renderer = Renderer::Secondary(iced_tiny_skia::Renderer::new(
        iced::Font::MONOSPACE,
        14.5.into(),
    ));
    event(
        &mut app,
        &mut tree,
        &renderer,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
    );
    event(
        &mut app,
        &mut tree,
        &renderer,
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    );
    app.selection = Selection {
        anchor: 15,
        head: 7,
    };
    app.sync_editor();
    assert_eq!(app.editor.selection(), app.selection);
    event(
        &mut app,
        &mut tree,
        &renderer,
        Event::InputMethod(input_method::Event::Opened),
    );
    event(
        &mut app,
        &mut tree,
        &renderer,
        Event::InputMethod(input_method::Event::Preedit("にほん".into(), Some(0..9))),
    );
    assert_eq!(app.document.source(), "Before selected after");
    event(
        &mut app,
        &mut tree,
        &renderer,
        Event::InputMethod(input_method::Event::Preedit(String::new(), None)),
    );
    let mut remote = Document::from_state(&app.document.collaboration.state()).unwrap();
    remote.replace(9..12, "remote").unwrap(); // partial change inside the composition replacement
    drop(app.update(Message::ReplicaLoaded(Ok(remote.collaboration.state()))));
    event(
        &mut app,
        &mut tree,
        &renderer,
        Event::InputMethod(input_method::Event::Commit("日本".into())),
    );
    assert_eq!(app.document.source().matches("日本").count(), 1);
    assert!(app.document.source().contains("remote"));
    assert_eq!(app.editor.snapshot(), app.document.source());
    drop(app.update(Message::Undo));
    assert_eq!(app.document.source(), remote.source());
    drop(app.update(Message::Redo));
    assert_eq!(app.document.source().matches("日本").count(), 1);
}

#[test]
fn crdt_sparse_import_keeps_reverse_selection_preedit_and_viewport() {
    use crate::document::Document;
    use crate::document::collaboration::{EditOrigin, SourceEdit};
    use crate::document::projection::Projection;
    let source = fixture(1000);
    let mut harness = Harness::new(source.clone());
    harness.frame(&mut Sample::default());
    harness.event(
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Point::new(80.0, 40.0),
    );
    harness.event(
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        Point::new(80.0, 40.0),
    );
    let mut peer = Document::from_state(&harness.app.document.collaboration.state()).unwrap();
    let selection = Selection {
        anchor: source.find("![A local").unwrap(),
        head: source.find("**bold").unwrap() + 2,
    };
    harness.message(Message::SelectSource(selection));
    let target = harness.app.document.collaboration.selection(selection);
    let edits = ["Paragraph 0.0", "Paragraph 30.0", "Paragraph 70.0"].map(|text| {
        let point = peer.source().find(text).unwrap();
        SourceEdit {
            range: point..point + text.len(),
            text: "日本 é 🧑‍💻".into(),
        }
    });
    peer.transact(&edits, EditOrigin::Human, None, true)
        .unwrap();
    harness.message(Message::ReplicaLoaded(Ok(peer.collaboration.state())));
    assert_eq!(
        harness.app.selection,
        harness
            .app
            .document
            .collaboration
            .resolve_selection(&target)
            .unwrap()
    );
    assert!(!harness.app.selection.is_caret());
    assert!(harness.app.selection.anchor > harness.app.selection.head);
    assert_eq!(
        harness
            .app
            .projection
            .change
            .as_ref()
            .unwrap()
            .regions
            .len(),
        3
    );
    assert!(!harness.app.document.can_undo());
    harness.message(Message::SelectSource(Selection::caret(
        harness.app.selection.head,
    )));
    let before = harness.app.document.source().to_owned();
    harness.event(
        Event::InputMethod(input_method::Event::Opened),
        Point::ORIGIN,
    );
    harness.event(
        Event::InputMethod(input_method::Event::Preedit("にほん".into(), Some(0..9))),
        Point::ORIGIN,
    );
    let edits = ["Paragraph 0.1", "Paragraph 30.1", "Paragraph 70.1"].map(|text| {
        let point = peer.source().find(text).unwrap();
        SourceEdit {
            range: point..point,
            text: "prefix 日 ".into(),
        }
    });
    peer.transact(&edits, EditOrigin::Human, None, true)
        .unwrap();
    harness.message(Message::ReplicaLoaded(Ok(peer.collaboration.state())));
    assert!(RichEditor::intent_stats(&harness.tree).1);
    assert_ne!(harness.app.document.source(), before);
    harness.event(
        Event::InputMethod(input_method::Event::Commit("確定 🦀".into())),
        Point::ORIGIN,
    );
    assert_eq!(harness.app.document.source().matches("確定 🦀").count(), 1);
    harness.message(Message::Undo);
    assert_eq!(harness.app.document.source(), peer.source());
    harness.message(Message::Redo);
    assert_eq!(harness.app.document.source().matches("確定 🦀").count(), 1);
    let fresh = Projection::with_context(
        harness.app.document.source(),
        Some(harness.app.selection.head),
    );
    assert_eq!(harness.app.projection.text, fresh.text);
    assert_eq!(harness.app.projection.rich, fresh.rich);
    assert_eq!(harness.app.projection.highlights, fresh.highlights);
    assert_eq!(harness.app.editor.snapshot(), fresh.text);

    // An offscreen caret must not pull the viewport back during sparse imports.
    harness.message(Message::SelectSource(Selection::caret(0)));
    harness.event(
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -1200.0 },
        }),
        Point::new(200.0, 200.0),
    );
    let visible = RichEditor::visible_anchor(&harness.tree).unwrap();
    let anchored = harness
        .app
        .document
        .collaboration
        .position(visible, yrs::Assoc::After);
    let y = RichEditor::screen_y(&harness.tree, visible);
    peer.receive(&harness.app.document.collaboration.state())
        .unwrap();
    let edits = ["Paragraph 0.2", "Paragraph 30.2"].map(|text| {
        let point = peer.source().find(text).unwrap();
        SourceEdit {
            range: point..point,
            text: "Another prefix ".into(),
        }
    });
    peer.transact(&edits, EditOrigin::Human, None, true)
        .unwrap();
    harness.message(Message::ReplicaLoaded(Ok(peer.collaboration.state())));
    let visible = harness
        .app
        .document
        .collaboration
        .resolve(&anchored)
        .unwrap();
    assert!((y - RichEditor::screen_y(&harness.tree, visible)).abs() < 1.0);
}

#[test]
#[ignore = "large offline batch shapes, including bounded incremental fallback"]
fn large_offline_import_shapes() {
    use crate::document::collaboration::{Collaboration, EditOrigin, SourceEdit};
    use crate::document::projection::Projection;
    load_profile_fonts();
    let samples = std::env::var("DAYTRIP_IMPORT_SAMPLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_usize)
        .max(1);
    for blocks in [10_000, 50_000] {
        for (name, count, inserted) in [
            ("sparse-32", 32, "日"),
            ("fallback-128", 128, "日"),
            ("structural-8", 8, "\n\n# Added heading\n\n"),
        ] {
            let mut harness = Harness::new(fixture(blocks));
            harness.frame(&mut Sample::default());
            let (mut peer, mut source) =
                Collaboration::from_state(&harness.app.document.collaboration.state()).unwrap();
            let mut timings = Vec::new();
            let mut certified = 0;
            for _ in 0..samples {
                let paragraphs: Vec<_> = source
                    .match_indices("Paragraph ")
                    .step_by(4)
                    .map(|(p, _)| p)
                    .collect();
                let edits: Vec<_> = (1..=count)
                    .map(|step| {
                        let paragraph = paragraphs[step * paragraphs.len() / (count + 1)];
                        let point = paragraph + source[paragraph..].find(':').unwrap() + 2;
                        SourceEdit {
                            range: point..point,
                            text: inserted.into(),
                        }
                    })
                    .collect();
                let committed = peer
                    .commit(&source, &edits, EditOrigin::Human, None, true)
                    .unwrap();
                for edit in committed.into_iter().rev() {
                    source.replace_range(edit.range, &edit.text);
                }
                timings.push(harness.message(Message::ReplicaLoaded(Ok(peer.state()))));
                certified += usize::from(!harness.app.document.block_changes.is_empty());
                assert_eq!(harness.app.document.source(), source);
                assert_eq!(harness.app.document.collaboration.materialize(), source);
                let fresh = Projection::with_context(&source, Some(harness.app.selection.head));
                assert_eq!(harness.app.projection.text, fresh.text);
                assert_eq!(harness.app.projection.rich, fresh.rich);
                assert_eq!(harness.app.projection.highlights, fresh.highlights);
                assert_eq!(harness.app.editor.snapshot(), fresh.text);
            }
            report(&format!("remote/{name}/{blocks}/first"), &timings[..1]);
            report(&format!("remote/{name}/{blocks}"), &timings);
            println!("import/windows blocks={blocks} shape={name} certified={certified}/{samples}");
            report_memory(&format!("remote/{name}/{blocks}"));
        }
    }
}
