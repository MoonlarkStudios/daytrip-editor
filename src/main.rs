//! Still is a native, local-first Markdown writing environment.

mod app;
mod document;
mod icon;
mod image_store;
mod markdown_view;
mod projection;
mod rich_document;
mod rich_editor;
mod rich_layout;
mod selection;
mod storage;
mod styles;
mod table_edit;
mod theme;
mod viewport;

use app::App;
use iced::Size;
use iced::window;
use tracing_subscriber::EnvFilter;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "still=info".into()))
        .init();

    iced::application(App::new, App::update, App::view)
        .font(include_bytes!("../assets/fonts/Newsreader-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Newsreader-Bold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Newsreader-Italic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Newsreader-BoldItalic.ttf").as_slice())
        .title(App::title)
        .theme(App::theme)
        .style(App::appearance)
        .subscription(App::subscription)
        .window(window::Settings {
            size: Size::new(1180.0, 820.0),
            min_size: Some(Size::new(760.0, 520.0)),
            exit_on_close_request: false,
            ..window::Settings::default()
        })
        .run()
}
