//! Application orchestration and the native entry point.

mod application;
pub(crate) mod icon;
mod startup;

pub(crate) use application::writing::Message as WritingMessage;
pub(crate) use application::{App, Message};
pub use startup::run;
