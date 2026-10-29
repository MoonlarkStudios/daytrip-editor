//! Footnote details and navigation over stable, checked Markdown source targets.

use super::{App, EDITOR, EditorMode, Message as AppMessage, Overlay};

use iced::widget::{column, container, markdown, operation, row, scrollable, text, text_editor};
use iced::{Element, Task};

use crate::document::collaboration::{EditOrigin, SourceEdit, StableSelection};
use crate::document::footnotes;
use crate::document::selection::Selection;
use crate::document::writing::{self, Target};
use crate::theme::styles;

const BODY: &str = "footnote-body";
const MAX_PREVIEW_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub enum Message {
    Details,
    OpenAt(usize),
    Body(text_editor::Action),
    Apply,
    Definition,
    Back,
}

struct Form {
    label: String,
    number: Option<usize>,
    target: Target,
    reference: Option<Target>,
    body: text_editor::Content,
    preview: Vec<markdown::Item>,
    existing: bool,
    ambiguous: bool,
    source_body: String,
    continuation_columns: usize,
    return_to: StableSelection,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct State {
    form: Option<Form>,
    back: Option<(u64, StableSelection)>,
}

impl State {
    pub fn close(&mut self) {
        self.form = None;
    }
    pub fn can_return(&self) -> bool {
        self.back.is_some()
    }
}

impl App {
    pub(super) fn update_footnote(&mut self, message: Message) -> Task<AppMessage> {
        match message {
            Message::Details | Message::OpenAt(_) => {
                if self.loading || self.overlay.is_some() {
                    return Task::none();
                }
                let offset = match message {
                    Message::OpenAt(offset) => offset,
                    _ => self.selection.head,
                };
                if offset > self.document.source().len() {
                    return Task::none();
                }
                self.open_footnote(offset);
                return operation::focus(BODY);
            }
            Message::Body(action) => {
                if let Some(form) = &mut self.footnotes.form {
                    form.body.perform(action);
                    form.preview = preview(&footnotes::display_body(
                        &form.body.text(),
                        form.continuation_columns,
                    ));
                    form.error = None;
                }
            }
            Message::Apply => {
                if self.loading || !matches!(self.overlay, Some(Overlay::Footnote)) {
                    return Task::none();
                }
                let Some(mut form) = self.footnotes.form.take() else {
                    return Task::none();
                };
                match self.apply_footnote(&form) {
                    Ok(()) => {
                        self.overlay = None;
                        return operation::focus(EDITOR);
                    }
                    Err(error) => {
                        form.error = Some(error);
                        self.footnotes.form = Some(form);
                    }
                }
            }
            Message::Definition => {
                let Some(form) = &self.footnotes.form else {
                    return Task::none();
                };
                if !form.existing {
                    return Task::none();
                }
                match form
                    .target
                    .resolve(&self.document.collaboration, self.document.source())
                {
                    Ok(range) => {
                        self.footnotes.back = Some((
                            self.document.collaboration.replica_id(),
                            form.return_to.clone(),
                        ));
                        self.footnotes.close();
                        self.overlay = None;
                        return self.footnote_jump(Selection::caret(range.start));
                    }
                    Err(error) => {
                        if let Some(form) = &mut self.footnotes.form {
                            form.error = Some(error)
                        }
                    }
                }
            }
            Message::Back => {
                if self.loading || self.overlay.is_some() {
                    return Task::none();
                }
                if let Some((owner, selection)) = self.footnotes.back.take()
                    && owner == self.document.collaboration.replica_id()
                    && let Some(selection) =
                        self.document.collaboration.resolve_selection(&selection)
                {
                    return self.footnote_jump(selection);
                }
            }
        }
        Task::none()
    }

    fn open_footnote(&mut self, offset: usize) {
        let source = self.document.source();
        let notes = self.projection.footnotes();
        let reference = notes
            .reference_at(offset)
            .or_else(|| notes.reference_before(offset));
        let definition = reference
            .and_then(|note| note.definition)
            .and_then(|index| notes.definitions.get(index))
            .or_else(|| notes.definition_at(offset));
        let label = reference
            .map(|note| note.label.clone())
            .or_else(|| definition.map(|note| note.label.clone()))
            .unwrap_or_else(|| notes.next_label());
        let range = definition.map(|note| note.body.clone()).unwrap_or_else(|| {
            let offset =
                reference.map_or(self.selection.range().end, |reference| reference.range.end);
            offset..offset
        });
        let body = definition.map_or("", |note| &source[note.body.clone()]);
        let continuation_columns =
            definition.map_or(0, |note| footnotes::continuation_columns(source, note));
        let return_selection = reference.map_or(self.selection, |note| {
            if note.range.contains(&self.selection.head) || self.selection.head == note.range.end {
                self.selection
            } else {
                Selection::caret(note.range.start)
            }
        });
        self.footnotes.form = Some(Form {
            label,
            number: reference
                .and_then(|note| note.number)
                .or_else(|| definition.and_then(|note| note.number)),
            target: Target::new(&self.document.collaboration, source, range),
            reference: reference
                .map(|note| Target::new(&self.document.collaboration, source, note.range.clone())),
            body: text_editor::Content::with_text(body),
            preview: preview(&footnotes::display_body(body, continuation_columns)),
            existing: definition.is_some(),
            ambiguous: definition.is_some_and(|note| note.ambiguous),
            source_body: body.to_owned(),
            continuation_columns,
            return_to: self.document.collaboration.selection(return_selection),
            error: None,
        });
        self.overlay = Some(Overlay::Footnote);
        self.document.break_history_group();
    }

    fn apply_footnote(&mut self, form: &Form) -> Result<(), String> {
        if form.ambiguous {
            return Err("Multiple definitions use this label. Go to the definition and resolve them in the document before editing these details.".into());
        }
        let source = self.document.source();
        let range = form.target.resolve(&self.document.collaboration, source)?;
        if let Some(reference) = &form.reference {
            reference.resolve(&self.document.collaboration, source)?;
        }
        let body = form.body.text();
        if form.existing && body == form.source_body {
            return Ok(());
        }
        if body.trim().is_empty() {
            return Err("Add the words for this footnote.".into());
        }
        let newline = writing::newline(source);
        let mut inserted_reference = None;
        let edits = if form.existing {
            let mut candidate = source.to_owned();
            candidate.replace_range(range.clone(), &body);
            let parsed = crate::document::projection::Projection::new(&candidate);
            if !parsed.footnotes().definitions.iter().any(|definition| {
                definition.body.start == range.start
                    && definition.body.end <= range.start + body.len()
                    && candidate[definition.body.end..range.start + body.len()]
                        .trim()
                        .is_empty()
                    && unicase::eq(&definition.label, &form.label)
                    && !definition.ambiguous
            }) {
                return Err("Keep continuation lines indented by four spaces so all of the text remains in this footnote.".into());
            }
            vec![SourceEdit { range, text: body }]
        } else {
            // A name can have appeared in an intervening replica edit. Never
            // silently create a second definition or retarget an existing note.
            let notes = self.projection.footnotes();
            if notes
                .definitions
                .iter()
                .any(|definition| unicase::eq(&definition.label, &form.label))
                || form.reference.is_none()
                    && notes
                        .references
                        .iter()
                        .any(|reference| unicase::eq(&reference.label, &form.label))
            {
                return Err("This label gained a definition while its details were open. Close and reopen the footnote.".into());
            }
            let suffix = if source.is_empty() || source.ends_with(&newline.repeat(2)) {
                String::new()
            } else if source.ends_with(newline) {
                newline.to_owned()
            } else {
                newline.repeat(2)
            };
            let definition = format!(
                "{suffix}{}",
                footnotes::definition(&form.label, &body, newline)
            );
            let mut edits = Vec::new();
            if form.reference.is_none() {
                let marker = format!("[^{}]", form.label);
                inserted_reference = Some((range.start, marker.len()));
                if range.start == source.len() {
                    edits.push(SourceEdit {
                        range,
                        text: format!(
                            "{marker}{}{}",
                            newline.repeat(2),
                            footnotes::definition(&form.label, &body, newline)
                        ),
                    });
                } else {
                    edits.push(SourceEdit {
                        range,
                        text: marker,
                    });
                    edits.push(SourceEdit {
                        range: source.len()..source.len(),
                        text: definition,
                    });
                }
            } else {
                edits.push(SourceEdit {
                    range: source.len()..source.len(),
                    text: definition,
                });
            }
            let mut candidate = source.to_owned();
            for edit in edits.iter().rev() {
                candidate.replace_range(edit.range.clone(), &edit.text);
            }
            let parsed = crate::document::projection::Projection::new(&candidate);
            if !parsed.footnotes().definitions.iter().any(|definition| {
                unicase::eq(&definition.label, &form.label) && !definition.ambiguous
            }) || inserted_reference.is_some_and(|(at, _)| {
                parsed
                    .footnotes()
                    .reference_at(at)
                    .is_none_or(|reference| !unicase::eq(&reference.label, &form.label))
            }) {
                return Err("Place the reference in prose and close any unfinished code fence before adding this note.".into());
            }
            edits
        };
        let mut after = selection_after(self.selection, &edits);
        if let Some((at, length)) = inserted_reference
            && self.selection.is_caret()
            && self.selection.head == at
        {
            after = Selection::caret(at + length);
        }
        self.document
            .transact(
                &edits,
                EditOrigin::Human,
                Some((self.selection, after)),
                true,
            )
            .map_err(|error| error.to_string())?;
        self.selection = after;
        self.preserve_viewport_generation = Some(self.document.collaboration.generation());
        self.sync_editor();
        self.changed();
        Ok(())
    }

    fn footnote_jump(&mut self, selection: Selection) -> Task<AppMessage> {
        if self.mode == EditorMode::Read {
            self.mode = EditorMode::Live;
        }
        self.selection = selection.clamp(self.document.source());
        self.editor_focused = true;
        self.document.break_history_group();
        self.sync_editor();
        operation::focus(EDITOR)
    }

    pub(super) fn footnote_view(&self) -> Element<'_, AppMessage> {
        let tokens = self.resolved_theme.tokens;
        let Some(form) = &self.footnotes.form else {
            return container(text("Footnote")).into();
        };
        let title = form.number.map_or_else(
            || "Footnote".to_owned(),
            |number| format!("Footnote {number}"),
        );
        let mut content = column![
            text(title).font(styles::BODY_FONT).size(28),
            text(format!("Markdown label: {}", form.label)).size(12).color(tokens.muted),
            text("Preview").size(12).color(tokens.muted),
            markdown::view(&form.preview, styles::markdown_settings(&self.resolved_theme)).map(AppMessage::Link),
            text(if form.existing { "Definition · keep continuation indentation" } else { "Note · Markdown is welcome" }).size(12).color(tokens.muted),
            text_editor(&form.body).id(BODY).height(180).padding(12).size(16)
                .on_action(|action| AppMessage::Footnote(Message::Body(action))),
            text("References use document-wide numbers. Definitions remain ordinary editable Markdown.").size(12).color(tokens.muted),
        ].spacing(14);
        if form.ambiguous {
            content = content.push(text("Multiple definitions use this label; edit the exact source to resolve the ambiguity.").size(13).color(tokens.accent));
        }
        if let Some(error) = &form.error {
            content = content.push(text(error).size(13).color(tokens.accent));
        }
        let mut actions =
            row![self.small_button("Cancel", AppMessage::CloseOverlay, false)].spacing(8);
        if form.existing {
            actions = actions.push(self.small_button(
                "Go to definition",
                AppMessage::Footnote(Message::Definition),
                false,
            ));
        }
        actions =
            actions.push(self.small_button("Apply", AppMessage::Footnote(Message::Apply), true));
        content = content.push(actions);
        container(
            scrollable(content)
                .height(iced::Shrink)
                .style(styles::scrollbar(tokens)),
        )
        .max_height(740)
        .width(550)
        .padding(28)
        .style(styles::card(tokens))
        .into()
    }
}

fn preview(body: &str) -> Vec<markdown::Item> {
    let mut end = body.len().min(MAX_PREVIEW_BYTES);
    while !body.is_char_boundary(end) {
        end -= 1
    }
    markdown::parse(&body[..end]).collect()
}

fn selection_after(selection: Selection, edits: &[SourceEdit]) -> Selection {
    let map = |offset: usize, after: bool| {
        let mut delta = 0isize;
        for edit in edits {
            if offset < edit.range.start || offset == edit.range.start && !after {
                break;
            }
            if offset < edit.range.end {
                return edit.range.start.saturating_add_signed(delta) + edit.text.len();
            }
            delta += edit.text.len() as isize - edit.range.len() as isize;
        }
        offset.saturating_add_signed(delta)
    };
    let forward = selection.anchor <= selection.head;
    Selection {
        anchor: map(selection.anchor, forward),
        head: map(selection.head, !forward || selection.is_caret()),
    }
}

#[cfg(test)]
#[path = "footnote_tests.rs"]
mod tests;
