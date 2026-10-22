//! Still's native application. Implementation modules remain crate-private;
//! the binary starts the application through the single `run` entry point.

mod app;
mod document;
mod editor;
mod io;
mod theme;

pub use app::run;
