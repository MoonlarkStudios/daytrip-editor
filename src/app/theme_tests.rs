//! Theme integration and full-window software previews. Never opens a window.

use super::*;
use iced::advanced::{Layout, Renderer as _, layout, mouse, renderer, widget};
use iced::{Rectangle, Renderer, Size};

#[test]
fn switching_appearance_preserves_the_document_and_reverse_selection() {
    let (mut app, _) = App::new();
    app.set_document(
        "# A thought\n\nKeep **these words**.\n".into(),
        None,
        "Thought.md".into(),
    );
    app.selection = Selection {
        anchor: 29,
        head: 16,
    };
    drop(app.update_message(Message::Format(Format::Bold)));
    assert!(app.document.can_undo());
    let source = app.document.source().to_owned();
    let selection = app.selection;
    let undo = app.document.can_undo();
    for preset in Preset::ALL {
        drop(app.update_message(Message::Theme(preset)));
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, selection);
        assert_eq!(app.document.can_undo(), undo);
        assert_eq!(
            app.theme().palette().background,
            app.resolved_theme.tokens.canvas
        );
    }
    let previous = app.resolved_theme.tokens;
    drop(app.update_message(Message::ThemeLoaded(Ok(Some((
        "invalid.toml".into(),
        "this is not a theme".into(),
    ))))));
    assert_eq!(app.resolved_theme.tokens, previous);
    assert_eq!(app.document.source(), source);
    assert_eq!(app.selection, selection);
    assert_eq!(app.document.can_undo(), undo);
}

fn render(app: &App, size: Size, name: &str) {
    let mut renderer =
        Renderer::Secondary(iced_tiny_skia::Renderer::new(Font::DEFAULT, 18.0.into()));
    let mut root = app.view();
    let mut tree = widget::Tree::new(root.as_widget());
    // Responsive widgets resolve their content during layout.
    let node =
        root.as_widget_mut()
            .layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, size));
    let clip = Rectangle::with_size(size);
    let mut messages = Vec::new();
    root.as_widget_mut().update(
        &mut tree,
        &Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        )),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut iced::advanced::clipboard::Null,
        &mut iced::advanced::Shell::new(&mut messages),
        &clip,
    );
    renderer.reset(clip);
    root.as_widget().draw(
        &tree,
        &mut renderer,
        &app.theme(),
        &renderer::Style {
            text_color: app.resolved_theme.tokens.text,
        },
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &clip,
    );
    let size = Size::new(size.width as u32, size.height as u32);
    let mut pixels = tiny_skia::Pixmap::new(size.width, size.height).unwrap();
    let mut mask = tiny_skia::Mask::new(size.width, size.height).unwrap();
    if let Renderer::Secondary(renderer) = &mut renderer {
        renderer.draw(
            &mut pixels.as_mut(),
            &mut mask,
            &iced_tiny_skia::graphics::Viewport::with_physical_size(size, 1.0),
            &[clip],
            app.resolved_theme.tokens.canvas,
        );
    }
    // Iced's software engine targets the native BGRA window surface.
    let mut rgba = pixels.take();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    image::save_buffer(
        format!("target/theme-validation/{name}.png"),
        &rgba,
        size.width,
        size.height,
        image::ColorType::Rgba8,
    )
    .unwrap();
}

#[test]
#[ignore = "render Koan themes, narrow windows, overlays, and focus mode for visual inspection"]
fn theme_ui_previews() {
    let mut fonts = iced_tiny_skia::graphics::text::font_system()
        .write()
        .unwrap();
    for font in [
        include_bytes!("../../assets/fonts/Newsreader-Regular.ttf").as_slice(),
        include_bytes!("../../assets/fonts/Newsreader-Bold.ttf").as_slice(),
        include_bytes!("../../assets/fonts/Newsreader-Italic.ttf").as_slice(),
        include_bytes!("../../assets/fonts/Newsreader-BoldItalic.ttf").as_slice(),
    ] {
        fonts.load_font(std::borrow::Cow::Borrowed(font));
    }
    drop(fonts);
    std::fs::create_dir_all("target/theme-validation").unwrap();
    let (mut app, _) = App::new();
    app.preferences.custom_theme = None;
    app.sidebar = true;
    app.focus = false;
    app.sidebar_tab = SidebarTab::Files;
    app.notification = None;
    app.preferences.recent.clear();
    for preset in Preset::ALL {
        app.resolved_theme = preset.resolve();
        render(&app, Size::new(1180.0, 820.0), preset.name());
        render(
            &app,
            Size::new(760.0, 520.0),
            &format!("{}-narrow", preset.name()),
        );
        app.overlay = Some(Overlay::Settings);
        render(
            &app,
            Size::new(1180.0, 820.0),
            &format!("{}-settings", preset.name()),
        );
        render(
            &app,
            Size::new(760.0, 520.0),
            &format!("{}-settings-narrow", preset.name()),
        );
        app.overlay = None;
    }
    app.resolved_theme = Preset::Paper.resolve();
    app.display_name = "Notes for a very long and unbroken document title that should never push controls out of the window.md".into();
    render(&app, Size::new(760.0, 520.0), "long-title-narrow");
    app.display_name = "Welcome.md".into();
    app.focus = true;
    render(&app, Size::new(1180.0, 820.0), "focus");
    app.focus = false;
    app.search_open = true;
    render(&app, Size::new(760.0, 520.0), "search-narrow");
    app.search_open = false;
    drop(app.update_message(Message::ToggleSource));
    render(&app, Size::new(1180.0, 820.0), "source");
    drop(app.update_message(Message::ToggleReading));
    render(&app, Size::new(1180.0, 820.0), "read");
    drop(app.update_message(Message::ToggleReading));
    app.dirty = true;
    app.overlay = Some(Overlay::Unsaved);
    render(&app, Size::new(760.0, 520.0), "unsaved-narrow");
    app.overlay = None;
    app.set_document(
        include_str!("../../tests/fixtures/theming.md").into(),
        None,
        "Code.md".into(),
    );
    for preset in Preset::ALL {
        app.resolved_theme = preset.resolve();
        render(
            &app,
            Size::new(1180.0, 820.0),
            &format!("{}-code", preset.name()),
        );
    }
}
