//! Native application startup and Iced configuration.

use super::App;
use iced::Size;
use iced::window;
use tracing_subscriber::EnvFilter;

/// Starts the native editor and runs its Iced event loop.
pub fn run() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "dosty=info".into()))
        .init();

    // Decode the bundled icon before entering the UI event loop.
    let icon = image::load_from_memory(include_bytes!(
        "../../branding/dosty/icons/png/dosty-128.png"
    ))
    .expect("the bundled Dosty icon must be a valid PNG")
    .into_rgba8();
    let icon = window::icon::from_rgba(icon.to_vec(), icon.width(), icon.height())
        .expect("the decoded Dosty icon must contain complete RGBA pixels");

    iced::application(App::new, App::update, App::view)
        .font(include_bytes!("../../assets/fonts/Newsreader-Regular.ttf").as_slice())
        .font(include_bytes!("../../assets/fonts/Newsreader-Bold.ttf").as_slice())
        .font(include_bytes!("../../assets/fonts/Newsreader-Italic.ttf").as_slice())
        .font(include_bytes!("../../assets/fonts/Newsreader-BoldItalic.ttf").as_slice())
        .title(App::title)
        .theme(App::theme)
        .style(App::appearance)
        .subscription(App::subscription)
        .window(window::Settings {
            size: Size::new(1180.0, 820.0),
            min_size: Some(Size::new(760.0, 520.0)),
            icon: Some(icon),
            exit_on_close_request: false,
            ..window::Settings::default()
        })
        .run()
}
