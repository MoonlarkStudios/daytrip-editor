//! Single-writer commands and focused details dialogs over authoritative source.

use super::{App, EDITOR, EditorMode, Message as AppMessage, Overlay, Pending};
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{column, container, operation, row, scrollable, text, text_editor, text_input};
use iced::{Element, Task};

use crate::document::collaboration::{
    CompositionTarget, EditOrigin, SourceEdit, StablePosition, StableSelection,
};
use crate::document::selection::Selection;
use crate::document::writing::{self, Details, Target};
use crate::io::{assets, storage};
use crate::theme::styles;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Link,
    Image,
    Quote,
}
#[derive(Debug, Clone, Copy)]
pub enum Field {
    Label,
    Destination,
    Caption,
    Credit,
    CreditUrl,
}
#[derive(Debug, Clone)]
pub enum Message {
    Details(Kind),
    Field(Field, String),
    Body(text_editor::Action),
    Accept,
    Remove,
    PickImage,
    ImageChosen {
        target: DeferredTarget,
        path: Option<PathBuf>,
        details: Option<Box<Details>>,
    },
    Paste(Option<String>),
    Clipboard {
        target: DeferredTarget,
        result: Result<assets::Clipboard, String>,
    },
    Imported {
        target: DeferredTarget,
        label: String,
        details: Option<Box<Details>>,
        result: Result<String, storage::FileError>,
    },
    ResumeImage,
    OpenAt(usize),
    ImageAt(usize),
    Sample,
}

/// Async paste/import insertion stays before text typed later at the same gap.
/// Nonempty replacement targets retain the IME target's overlap protection.
#[derive(Debug, Clone)]
pub struct DeferredTarget {
    composition: CompositionTarget,
    gap: Option<StablePosition>,
    selection: Selection,
    generation: u64,
}
impl DeferredTarget {
    fn new(app: &App, selection: Selection) -> Self {
        Self {
            composition: app.document.collaboration.composition_target(selection),
            gap: selection.is_caret().then(|| {
                app.document
                    .collaboration
                    .position(selection.head, yrs::Assoc::Before)
            }),
            selection,
            generation: app.document.collaboration.generation(),
        }
    }
    fn resolve(&self, app: &App) -> Option<Selection> {
        let selection = app
            .document
            .collaboration
            .resolve_composition(&self.composition)?;
        self.gap.as_ref().map_or(Some(selection), |gap| {
            app.document
                .collaboration
                .resolve(gap)
                .map(Selection::caret)
        })
    }
    fn displaced_cursor(&self, app: &App) -> Option<StableSelection> {
        (self.generation != app.document.collaboration.generation()
            || self.selection != app.selection)
            .then(|| app.document.collaboration.selection(app.selection))
    }
}

struct Form {
    kind: Kind,
    target: Target,
    details: Details,
    body: text_editor::Content,
    existing: bool,
    error: Option<String>,
}
pub(super) struct Media {
    target: DeferredTarget,
    input: assets::Input,
    label: String,
    details: Option<Box<Details>>,
}
#[derive(Default)]
pub(super) struct State {
    form: Option<Form>,
    pub pending_media: Option<Media>,
}

impl State {
    pub fn close_details(&mut self) {
        self.form = None;
    }
}

impl App {
    pub(super) fn update_writing(&mut self, message: Message) -> Task<AppMessage> {
        match message {
            Message::Details(kind) => {
                if self.loading || self.overlay.is_some() {
                    return Task::none();
                }
                if kind == Kind::Link
                    && (self.document.source()[self.selection.range()].contains("\n\n")
                        || self.document.source()[self.selection.range()].contains("\r\n\r\n"))
                {
                    self.notification =
                        Some("Select a phrase within one paragraph to make a link.".into());
                    return Task::none();
                }
                self.open_details(kind);
                return operation::focus("writing-details-first");
            }
            Message::Field(field, value) => {
                if let Some(form) = &mut self.writing.form {
                    match field {
                        Field::Label => form.details.label = value,
                        Field::Destination => form.details.destination = value,
                        Field::Caption => form.details.caption = value,
                        Field::Credit => form.details.credit = value,
                        Field::CreditUrl => form.details.credit_url = value,
                    }
                    form.error = None;
                }
            }
            Message::Body(action) => {
                if let Some(form) = &mut self.writing.form {
                    form.body.perform(action);
                }
            }
            Message::Accept | Message::Remove => {
                if !matches!(self.overlay, Some(Overlay::Writing)) {
                    return Task::none();
                }
                let remove = matches!(message, Message::Remove);
                let Some(mut form) = self.writing.form.take() else {
                    return Task::none();
                };
                let result = form
                    .target
                    .resolve(&self.document.collaboration, self.document.source())
                    .and_then(|range| {
                        if !remove {
                            let destination = if form.kind == Kind::Quote {
                                &form.details.credit_url
                            } else {
                                &form.details.destination
                            };
                            if !(writing::safe_destination(destination)
                                || form.kind == Kind::Quote && destination.trim().is_empty())
                            {
                                return Err(
                                    "Enter a web, email, document, or #heading destination.".into(),
                                );
                            }
                            if form.kind == Kind::Image
                                && (destination.starts_with("mailto:")
                                    || destination.starts_with('#'))
                            {
                                return Err(
                                    "Use a local image path or an HTTP/HTTPS image URL.".into()
                                );
                            }
                            if !form.details.credit_url.trim().is_empty()
                                && !writing::safe_destination(&form.details.credit_url)
                            {
                                return Err("Enter a valid source or credit destination.".into());
                            }
                            if form.kind == Kind::Quote && form.body.text().trim().is_empty() {
                                return Err("Add the words you want to quote.".into());
                            }
                            if form.kind == Kind::Image
                                && !form.details.credit_url.trim().is_empty()
                                && form.details.credit.trim().is_empty()
                            {
                                return Err(
                                    "Add a photographer or source name for this credit.".into()
                                );
                            }
                            if form.kind != Kind::Quote && form.details.label.trim().is_empty() {
                                return Err(
                                    "Add readable link text or describe what the image shows."
                                        .into(),
                                );
                            }
                        }
                        let newline = writing::newline(self.document.source());
                        let replacement = if remove {
                            if form.kind == Kind::Image {
                                String::new()
                            } else {
                                writing::escape(&form.details.label)
                            }
                        } else {
                            match form.kind {
                                Kind::Link => writing::details_link(&form.details),
                                Kind::Image => writing::figure(&form.details, newline),
                                Kind::Quote => writing::quote(
                                    &form.body.text(),
                                    &form.details.credit,
                                    &form.details.label,
                                    &form.details.credit_url,
                                    newline,
                                ),
                            }
                        };
                        let replacement = if !form.existing && form.kind != Kind::Link {
                            self.block_insertion(range.clone(), &replacement)
                        } else {
                            replacement
                        };
                        self.apply_writing(range, replacement)
                    });
                if let Err(error) = result {
                    form.error = Some(error);
                    self.writing.form = Some(form);
                } else {
                    self.overlay = None;
                    return operation::focus(EDITOR);
                }
            }
            Message::PickImage => {
                if self.loading || self.busy || !matches!(self.overlay, Some(Overlay::Writing)) {
                    return Task::none();
                }
                let selection = self
                    .writing
                    .form
                    .as_ref()
                    .and_then(|form| {
                        form.target
                            .resolve(&self.document.collaboration, self.document.source())
                            .ok()
                    })
                    .map_or(self.selection, |range| Selection {
                        anchor: range.start,
                        head: range.end,
                    });
                let target = DeferredTarget::new(self, selection);
                let details = self.writing.form.take().map(|form| Box::new(form.details));
                self.overlay = None;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    move |path| {
                        AppMessage::Writing(Message::ImageChosen {
                            target: target.clone(),
                            path,
                            details: details.clone(),
                        })
                    },
                );
            }
            Message::ImageChosen {
                target,
                path: Some(path),
                details,
            } => {
                let label = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .replace(['-', '_'], " ");
                return self.import_image(Media {
                    target,
                    label,
                    input: assets::Input::File(path),
                    details,
                });
            }
            Message::ImageChosen { path: None, .. } => {}
            Message::Paste(fallback) => {
                if self.loading || self.overlay.is_some() || self.mode == EditorMode::Read {
                    return Task::none();
                }
                let target = DeferredTarget::new(self, self.selection);
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || assets::clipboard(fallback))
                            .await
                            .map_err(|e| e.to_string())?
                    },
                    move |result| {
                        AppMessage::Writing(Message::Clipboard {
                            target: target.clone(),
                            result,
                        })
                    },
                );
            }
            Message::Clipboard { target, result } => {
                if self.loading || self.overlay.is_some() {
                    return Task::none();
                }
                match result {
                    Ok(assets::Clipboard::Image(input)) => {
                        return self.import_image(Media {
                            target,
                            input,
                            label: "Image".into(),
                            details: None,
                        });
                    }
                    Ok(assets::Clipboard::Text(value)) => {
                        if let Some(selection) = target.resolve(self) {
                            let cursor = target.displaced_cursor(self);
                            self.selection = selection;
                            self.sync_editor();
                            if !selection.is_caret()
                                && (value.starts_with("https://")
                                    || value.starts_with("http://")
                                    || value.starts_with("mailto:"))
                                && !self.document.source()[selection.range()].contains("\n\n")
                                && !self.document.source()[selection.range()].contains("\r\n\r\n")
                                && writing::safe_destination(&value)
                            {
                                let label = self.editor.content().selection().unwrap_or_default();
                                if let Err(error) = self
                                    .apply_writing(selection.range(), writing::link(&label, &value))
                                {
                                    self.notification = Some(error);
                                }
                                self.restore_deferred_cursor(cursor);
                                return self.deferred_focus();
                            }
                            let task = self.edit(text_editor::Action::Edit(
                                text_editor::Edit::Paste(Arc::new(value)),
                            ));
                            self.restore_deferred_cursor(cursor);
                            return task;
                        }
                    }
                    Ok(assets::Clipboard::Empty) => {}
                    Err(error) => self.notification = Some(error),
                }
            }
            Message::Imported {
                target,
                label,
                details,
                result,
            } => {
                self.busy = false;
                match result {
                    Ok(destination) => {
                        if let Some(selection) = target.resolve(self) {
                            let cursor = target.displaced_cursor(self);
                            let mut details = details.unwrap_or_default();
                            if details.label.trim().is_empty() {
                                details.label = label;
                            }
                            details.destination = destination;
                            let value =
                                writing::figure(&details, writing::newline(self.document.source()));
                            let value = self.block_insertion(selection.range(), &value);
                            if let Err(error) = self.apply_writing(selection.range(), value) {
                                self.notification = Some(error);
                            } else {
                                self.notification = Some("Image added. Use Image to add alt text, a caption, and a credit.".into());
                            }
                            self.restore_deferred_cursor(cursor);
                            return self.deferred_focus();
                        }
                    }
                    Err(error) => self.notification = Some(error.to_string()),
                }
            }
            Message::ResumeImage => {
                if let Some(media) = self.writing.pending_media.take() {
                    return self.import_image(media);
                }
            }
            Message::ImageAt(offset) => {
                if self.overlay.is_none() && !self.loading {
                    self.selection = Selection::caret(offset);
                    self.open_details(Kind::Image);
                    return operation::focus("writing-details-first");
                }
            }
            Message::OpenAt(offset) => {
                if self.overlay.is_none()
                    && let Some(details) = writing::inline_at(self.document.source(), offset, false)
                {
                    return self.open_destination(details.destination);
                }
            }
            Message::Sample => return self.request(Pending::Sample),
        }
        Task::none()
    }

    fn open_details(&mut self, kind: Kind) {
        let source = self.document.source();
        let quote = (kind == Kind::Quote)
            .then(|| writing::quote_at(source, self.selection.head))
            .flatten();
        let found = match kind {
            Kind::Link | Kind::Image => {
                writing::inline_at(source, self.selection.head, kind == Kind::Image)
            }
            Kind::Quote => quote.as_ref().map(|(details, _)| details.clone()),
        };
        let found = found.filter(|details| {
            self.selection.is_caret()
                || (details.range.start <= self.selection.range().start
                    && self.selection.range().end <= details.range.end)
        });
        let existing = found.is_some();
        let details = found.unwrap_or_else(|| Details {
            range: self.selection.range(),
            label: if kind == Kind::Quote {
                String::new()
            } else {
                self.editor.content().selection().unwrap_or_default()
            },
            ..Details::default()
        });
        let body = if kind == Kind::Quote {
            quote
                .as_ref()
                .map_or(&source[details.range.clone()], |(_, body)| body.as_str())
        } else {
            ""
        };
        self.writing.form = Some(Form {
            target: Target::new(&self.document.collaboration, source, details.range.clone()),
            details,
            body: text_editor::Content::with_text(body),
            existing,
            kind,
            error: None,
        });
        self.overlay = Some(Overlay::Writing);
        self.document.break_history_group();
    }

    pub(super) fn import_dropped_image(&mut self, path: PathBuf) -> Task<AppMessage> {
        if self.overlay.is_some() || self.loading {
            return Task::none();
        }
        if self.busy {
            self.notification =
                Some("Add images one at a time; finish the current file operation first.".into());
            return Task::none();
        }
        let target = DeferredTarget::new(self, self.selection);
        self.update_writing(Message::ImageChosen {
            target,
            path: Some(path),
            details: None,
        })
    }
    fn import_image(&mut self, media: Media) -> Task<AppMessage> {
        if self.busy {
            self.notification =
                Some("Finish the current file operation before adding another image.".into());
            return Task::none();
        }
        if media.target.resolve(self).is_none() {
            return Task::none();
        }
        let Some(path) = self.path.clone() else {
            self.writing.pending_media = Some(media);
            self.notification = Some("Save once to keep images beside your Markdown.".into());
            return self.save(false);
        };
        self.busy = true;
        let Media {
            target,
            input,
            label,
            details,
        } = media;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || assets::import(&path, input))
                    .await
                    .map_err(|e| storage::FileError::Io(e.to_string()))?
            },
            move |result| {
                AppMessage::Writing(Message::Imported {
                    target: target.clone(),
                    label: label.clone(),
                    details: details.clone(),
                    result,
                })
            },
        )
    }

    fn deferred_focus(&self) -> Task<AppMessage> {
        // A background import must not steal a dialog/search field's focus or
        // cancel its transient input-method composition.
        if self.overlay.is_none() && !self.search_open {
            operation::focus(EDITOR)
        } else {
            Task::none()
        }
    }

    fn restore_deferred_cursor(&mut self, cursor: Option<StableSelection>) {
        if let Some(cursor) = cursor
            && let Some(selection) = self.document.collaboration.resolve_selection(&cursor)
        {
            self.selection = selection;
            self.sync_editor();
        }
    }

    fn block_insertion(&self, range: std::ops::Range<usize>, text: &str) -> String {
        let newline = writing::newline(self.document.source());
        let prefix = &self.document.source()[..range.start];
        let suffix = &self.document.source()[range.end..];
        format!(
            "{}{text}{}",
            if prefix.is_empty() || prefix.ends_with(&newline.repeat(2)) {
                String::new()
            } else if prefix.ends_with(newline) {
                newline.into()
            } else {
                newline.repeat(2)
            },
            if suffix.starts_with(&newline.repeat(2)) {
                String::new()
            } else if suffix.starts_with(newline) {
                newline.into()
            } else {
                newline.repeat(2)
            }
        )
    }
    fn apply_writing(&mut self, range: std::ops::Range<usize>, text: String) -> Result<(), String> {
        let after = Selection::caret(range.start + text.len());
        self.document
            .transact(
                &[SourceEdit { range, text }],
                EditOrigin::Human,
                Some((self.selection, after)),
                true,
            )
            .map_err(|e| e.to_string())?;
        self.selection = after;
        self.mode = if self.mode == EditorMode::Read {
            EditorMode::Live
        } else {
            self.mode
        };
        self.preserve_viewport_generation = Some(self.document.collaboration.generation());
        self.sync_editor();
        self.changed();
        Ok(())
    }

    pub(super) fn open_destination(&mut self, destination: String) -> Task<AppMessage> {
        if !writing::safe_destination(&destination) {
            self.notification = Some("This link uses an unsupported destination.".into());
            return Task::none();
        }
        if let Some(fragment) = destination.strip_prefix('#') {
            let fragment = percent_encoding::percent_decode_str(fragment).decode_utf8_lossy();
            if let Some(index) = self.document.blocks.iter().position(|block| {
                block
                    .heading
                    .as_ref()
                    .is_some_and(|(_, label)| writing::heading_slug(label) == fragment)
            }) {
                self.overlay = None;
                self.writing.close_details();
                return self.activate(index, false);
            }
            self.notification = Some("No heading matches this link.".into());
        } else if destination.starts_with("https://")
            || destination.starts_with("http://")
            || destination.starts_with("mailto:")
        {
            return Task::perform(
                async move { open::that_detached(destination).map_err(|e| e.to_string()) },
                AppMessage::LinkOpened,
            );
        } else {
            let base = self.path.as_deref().and_then(|path| path.parent());
            match assets::local_path(&destination, base) {
                Ok(Some(path))
                    if path
                        .extension()
                        .is_some_and(|ext| ext == "md" || ext == "markdown" || ext == "txt") =>
                {
                    self.overlay = None;
                    self.writing.close_details();
                    return self.request(Pending::Open(Some(path)));
                }
                _ => self.notification = Some(
                    "Only Markdown documents, web, email, and #heading links can be opened here."
                        .into(),
                ),
            }
        }
        Task::none()
    }

    pub(super) fn writing_view(&self) -> Element<'_, AppMessage> {
        let tokens = self.resolved_theme.tokens;
        let Some(form) = &self.writing.form else {
            return container(text("Details")).into();
        };
        let title = match form.kind {
            Kind::Link => "Link",
            Kind::Image => "Image",
            Kind::Quote => "Attributed quotation",
        };
        let input = |label: &'static str,
                     placeholder: &'static str,
                     value: &str,
                     field: Field,
                     first: bool| {
            let mut input = text_input(placeholder, value)
                .on_input(move |value| AppMessage::Writing(Message::Field(field, value)))
                .on_submit(AppMessage::Writing(Message::Accept))
                .padding(10)
                .size(14)
                .style(styles::input(tokens));
            if first {
                input = input.id("writing-details-first");
            }
            column![text(label).size(12).color(tokens.muted), input].spacing(5)
        };
        let mut content = column![text(title).font(styles::BODY_FONT).size(28)].spacing(16);
        if form.kind == Kind::Quote {
            content = content.push(
                column![
                    text("Quotation · Markdown is welcome")
                        .size(12)
                        .color(tokens.muted),
                    text_editor(&form.body)
                        .on_action(|action| AppMessage::Writing(Message::Body(action)))
                        .height(140)
                        .padding(12)
                        .size(16)
                ]
                .spacing(5),
            );
            content = content.push(input(
                "Author",
                "Who said it?",
                &form.details.credit,
                Field::Credit,
                true,
            ));
            content = content.push(input(
                "Source title",
                "Book, essay, or source",
                &form.details.label,
                Field::Label,
                false,
            ));
            content = content.push(input(
                "Source destination · optional",
                "https://… or #heading",
                &form.details.credit_url,
                Field::CreditUrl,
                false,
            ));
        } else {
            content = content.push(input(
                if form.kind == Kind::Image {
                    "Alt text · describe what the image shows"
                } else {
                    "Link text"
                },
                "Readable text",
                &form.details.label,
                Field::Label,
                true,
            ));
            content = content.push(input(
                "Destination",
                if form.kind == Kind::Image {
                    "Image URL or local path"
                } else {
                    "https://… · email · document.md · #heading"
                },
                &form.details.destination,
                Field::Destination,
                false,
            ));
            if form.kind == Kind::Image {
                content = content.push(input(
                    "Caption · optional",
                    "A little context",
                    &form.details.caption,
                    Field::Caption,
                    false,
                ));
                content = content.push(input(
                    "Photo credit · optional",
                    "Photographer or source",
                    &form.details.credit,
                    Field::Credit,
                    false,
                ));
                content = content.push(input(
                    "Credit destination · optional",
                    "https://…",
                    &form.details.credit_url,
                    Field::CreditUrl,
                    false,
                ));
            }
        }
        content = content.push(
            text(match form.kind {
                Kind::Link => {
                    "Click to edit. ⌘ / Ctrl-click to open. Destinations stay visible here."
                }
                Kind::Image => {
                    "Images fit the writing column. Captions and credits stay plain Markdown."
                }
                Kind::Quote => "The author and source become a credit line inside the quotation.",
            })
            .size(12)
            .color(tokens.muted),
        );
        if let Some(error) = &form.error {
            content = content.push(text(error).size(13).color(tokens.accent));
        }
        let mut actions =
            row![self.small_button("Cancel", AppMessage::CloseOverlay, false)].spacing(8);
        if form.kind == Kind::Image {
            actions = actions.push(self.small_button(
                "Choose image…",
                AppMessage::Writing(Message::PickImage),
                false,
            ));
        }
        if form.kind == Kind::Link && !form.details.destination.is_empty() {
            actions = actions.push(self.small_button(
                "Open",
                AppMessage::Link(form.details.destination.clone()),
                false,
            ));
        }
        if form.existing {
            actions = actions.push(self.small_button(
                if form.kind == Kind::Image {
                    "Remove image"
                } else {
                    "Unlink"
                },
                AppMessage::Writing(Message::Remove),
                false,
            ));
        }
        actions =
            actions.push(self.small_button("Apply", AppMessage::Writing(Message::Accept), true));
        content = content.push(actions);
        container(scrollable(content).height(iced::Shrink))
            .max_height(740)
            .width(550)
            .padding(28)
            .style(styles::panel(tokens.surface))
            .into()
    }
}

#[cfg(test)]
#[path = "writing_tests.rs"]
mod tests;
