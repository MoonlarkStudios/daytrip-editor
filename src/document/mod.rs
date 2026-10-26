//! Authoritative source, selections, Markdown projection, and source edits.

pub(crate) mod block_parser;
pub(crate) mod code_edit;
pub(crate) mod collaboration;
mod collaboration_wire;
pub(crate) mod outline;
pub(crate) mod projection;
pub(crate) mod rich_document;
pub(crate) mod selection;
mod source;
pub(crate) mod table_edit;
pub(crate) mod writing;

pub(crate) use source::Document;
