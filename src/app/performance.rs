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
            &app.editor,
            &app.projection,
            app.document.source(),
            &app.resolved_theme,
            &app.images,
            Binding::from_key_press,
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
            ..Sample::default()
        };
        self.frame(&mut sample);
        sample
    }
}

fn report(name: &str, samples: &[Sample]) {
    type Phase = (&'static str, fn(Sample) -> Duration);
    let fields: [Phase; 6] = [
        ("input", |s| s.input),
        ("update", |s| s.update),
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

#[test]
#[ignore = "run with mise run profile; large documents and software rasterization"]
fn large_document_interactions() {
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
    let counts = std::env::var("STILL_PROFILE_BLOCKS").unwrap_or_else(|_| "10000,50000".into());
    let samples = std::env::var("STILL_PROFILE_SAMPLES")
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
        if std::env::var_os("STILL_PROFILE_COLD_ONLY").is_some() {
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
    let mut content = native_buffer::with_text(&projection.text);
    println!("load/native-init {:?}", start.elapsed());
    let start = Instant::now();
    Selection::caret(0).restore(&mut content, &projection.text);
    println!("load/native-caret {:?}", start.elapsed());
    assert_eq!(document.blocks.len(), 50_000);
    std::hint::black_box(content);
}
