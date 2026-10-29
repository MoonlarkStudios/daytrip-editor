//! Application orchestration and the native entry point.

mod application;
mod brand;
pub(crate) mod icon;
mod startup;

pub(crate) use application::footnotes::Message as FootnoteMessage;
pub(crate) use application::outline::Message as OutlineMessage;
pub(crate) use application::writing::Message as WritingMessage;
pub(crate) use application::{App, Message};
pub use startup::run;
