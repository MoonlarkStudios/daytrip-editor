//! Application state, commands, and the single-surface writing interface.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use iced::keyboard::{self, Key, key};
use iced::widget::{
    button, column, container, markdown, mouse_area, operation, responsive, row, rule, scrollable,
    space, stack, text, text_editor, text_input, tooltip,
};
use iced::window;
use iced::{Alignment, Element, Event, Fill, Font, Shrink, Subscription, Task, Theme};

use crate::app::brand;
use crate::app::icon::{self, Icon};
use crate::document::Document;
use crate::document::collaboration::{AgentProposal, CompositionTarget, EditOrigin, ReplicaState};
use crate::document::projection::{self, Projection};
use crate::document::selection::Selection;
use crate::document::table_edit;
use crate::editor::markdown_view::MarkdownView;
use crate::editor::native_buffer;
use crate::editor::rich_editor::RichEditor;
use crate::editor::source_editor::SourceEditor;
use crate::editor::viewport;
use crate::io::collaboration_store;
use crate::io::image_store::{ImageStore, LoadedImage};
use crate::io::storage::{self, FileError, LoadedFile, Preferences};
use crate::theme::styles::{self, Button};
use crate::theme::{Preset, ResolvedTheme};

#[path = "outline.rs"]
pub(crate) mod outline;
#[path = "recovery.rs"]
pub(crate) mod recovery;
#[path = "writing.rs"]
pub(crate) mod writing;

const EDITOR: &str = "document-editor";
const SCROLL: &str = "document-scroll";
const SEARCH: &str = "document-search";

#[cfg(test)]
#[path = "performance.rs"]
mod performance;

#[cfg(test)]
#[path = "theme_tests.rs"]
mod theme_tests;

#[cfg(test)]
#[path = "task_tests.rs"]
mod task_tests;

pub struct App {
    document: Document,
    editor: native_buffer::Buffer,
    native_edited: bool,
    edit_error: Option<String>,
    edit_rollback: Option<Selection>,
    preserve_viewport_generation: Option<u64>,
    editor_source: bool,
    active: Option<usize>,
    mode: EditorMode,
    projection: Projection,
    projection_revision: Option<u64>,
    editor_focused: bool,
    images: ImageStore,
    selection: Selection,
    path: Option<PathBuf>,
    display_name: String,
    saved_source: String,
    saved_version: Option<String>,
    saved_generation: u64,
    dirty: bool,
    busy: bool,
    loading: bool,
    resolved_theme: ResolvedTheme,
    preset_themes: [ResolvedTheme; 3],
    preferences: Preferences,
    preferences_writing: bool,
    preferences_pending: bool,
    sidebar: bool,
    sidebar_tab: SidebarTab,
    focus: bool,
    overlay: Option<Overlay>,
    pending: Option<Pending>,
    search_open: bool,
    query: String,
    replacement: String,
    matches: Vec<Range<usize>>,
    match_index: usize,
    match_revealed: bool,
    notification: Option<String>,
    zoom: f32,
    writing: writing::State,
    recovery: recovery::State,
    outline: outline::State,
}

#[derive(Debug, Clone)]
pub enum Message {
    Outline(outline::Message),
    Writing(writing::Message),
    Recovery(recovery::Message),
    Edit(text_editor::Action),
    CopyCode(String),
    CommitComposition {
        target: CompositionTarget,
        text: String,
    },
    ShowCollaboration,
    ExportReplica,
    OpenReplica,
    ReplicaChosen(Option<PathBuf>),
    ReplicaOpened(Result<ReplicaState, FileError>),
    ExportAgentSnapshot,
    ImportReplica,
    ImportProposal,
    ReplicaLoaded(Result<ReplicaState, FileError>),
    ProposalLoaded(Result<AgentProposal, FileError>),
    CollaborationFinished(Result<(), FileError>),
    AgentUndo,
    AgentRedo,
    Select(Selection),
    SelectSource(Selection),
    EditorFocused(bool),
    TableTab(bool),
    ToggleCurrentTask,
    ToggleTaskSource {
        marker: Range<usize>,
        revision: Arc<()>,
    },
    ImageLoaded {
        key: String,
        result: Result<LoadedImage, String>,
    },
    Activate(usize),
    JumpTo(usize),
    ToggleTask {
        block: usize,
        task: usize,
    },
    Append,
    Navigate(i32),
    Format(Format),
    Undo,
    Redo,
    New,
    Open,
    OpenPath(PathBuf),
    Guide,
    Welcome,
    Save,
    SaveAs,
    ChosenFile(Option<PathBuf>),
    Loaded(Result<LoadedFile, FileError>),
    Saved {
        result: Result<PathBuf, FileError>,
        source: Arc<String>,
        version: String,
        generation: u64,
    },
    Export,
    Exported(Result<Option<PathBuf>, FileError>),
    ToggleSource,
    ToggleReading,
    ToggleSidebar,
    SidebarTab(SidebarTab),
    ToggleFocus,
    ShowSettings,
    CloseOverlay,
    Theme(Preset),
    LoadTheme,
    ThemeLoaded(Result<Option<(PathBuf, String)>, FileError>),
    PreferencesSaved(Result<(), FileError>),
    ToggleSearch,
    Query(String),
    Replacement(String),
    FindNext,
    FindPrevious,
    Replace,
    ReplaceAll,
    ConfirmSave,
    ConfirmDiscard,
    CloseRequested(window::Id),
    Link(String),
    LinkOpened(Result<(), String>),
    Dismiss,
    Zoom(f32),
    Dropped(PathBuf),
}

#[derive(Debug, Clone, Copy)]
pub enum Format {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Heading,
    Quote,
    List,
    Task,
    Link,
    Fence,
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Live,
    Source,
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Files,
    Outline,
}

#[derive(Debug, Clone, Copy)]
enum Overlay {
    QuickJump,
    Settings,
    Collaboration,
    Unsaved,
    Writing,
    Recovery,
}

#[derive(Debug, Clone)]
enum Pending {
    New,
    Open(Option<PathBuf>),
    Replica,
    Guide,
    Welcome,
    Sample,
    Recover(Box<crate::io::recovery::Snapshot>),
    Close(window::Id),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let preferences = storage::load_preferences();
        let preset = Preset::ALL
            .into_iter()
            .find(|preset| preset.name() == preferences.theme)
            .unwrap_or(Preset::Paper);
        let preset_themes = Preset::ALL.map(Preset::resolve);
        let resolved_theme = preset_themes
            .iter()
            .find(|theme| theme.name == preset.name())
            .cloned()
            .unwrap_or_else(|| preset_themes[0].clone());
        let source = include_str!("../../assets/welcome.md").to_owned();
        let projection = Projection::new(&source);
        let mut app = Self {
            document: Document::new(source.clone()),
            editor: native_buffer::Buffer::new(&projection.text),
            native_edited: false,
            edit_error: None,
            edit_rollback: None,
            preserve_viewport_generation: None,
            editor_source: false,
            active: None,
            mode: EditorMode::Live,
            images: ImageStore::default(),
            projection,
            projection_revision: Some(0),
            editor_focused: false,
            selection: Selection::default(),
            path: None,
            display_name: "Welcome.md".into(),
            saved_source: source,
            saved_version: None,
            saved_generation: 0,
            dirty: false,
            busy: false,
            loading: false,
            resolved_theme,
            preset_themes,
            preferences,
            preferences_writing: false,
            preferences_pending: false,
            sidebar: true,
            sidebar_tab: SidebarTab::Files,
            focus: false,
            overlay: None,
            pending: None,
            search_open: false,
            query: String::new(),
            replacement: String::new(),
            matches: Vec::new(),
            match_index: 0,
            match_revealed: false,
            notification: None,
            zoom: 1.0,
            writing: writing::State::default(),
            recovery: recovery::State::default(),
            outline: outline::State::default(),
        };
        app.sync_editor();
        app.selection = app.projection.normalize_caret(app.selection);
        let theme_task = if let Some(path) = app.preferences.custom_theme.clone() {
            Task::perform(
                async move {
                    let source = tokio::fs::read_to_string(&path).await?;
                    Ok(Some((path, source)))
                },
                Message::ThemeLoaded,
            )
        } else {
            Task::none()
        };
        let open_task = std::env::args_os()
            .nth(1)
            .map(PathBuf::from)
            .map(|path| Task::done(Message::OpenPath(path)))
            .unwrap_or_else(Task::none);
        let image_task = app.image_tasks();
        let recovery_task = app.recovery_discovery();
        (
            app,
            Task::batch([theme_task, open_task, image_task, recovery_task]),
        )
    }

    pub fn title(&self) -> String {
        format!(
            "{}{} — Dosty",
            if self.dirty { "● " } else { "" },
            self.display_name
        )
    }

    /// The theme is resolved only on load/switch. Rendering clones the stored handle.
    pub fn theme(&self) -> Theme {
        self.resolved_theme.iced.clone()
    }

    pub fn appearance(&self, _: &Theme) -> iced::theme::Style {
        iced::theme::Style {
            background_color: self.resolved_theme.tokens.canvas,
            text_color: self.resolved_theme.tokens.text,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            if self.dirty
                && !self.loading
                && self
                    .recovery
                    .needs_checkpoint(self.document.collaboration.generation())
            {
                iced::time::every(std::time::Duration::from_secs(1))
                    .map(|_| Message::Recovery(recovery::Message::Tick))
            } else {
                Subscription::none()
            },
            iced::event::listen_with(event_message),
        ])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        #[cfg(test)]
        {
            self.document.costs = Default::default();
        }
        let task = self.update_message(message);
        Task::batch([task, self.image_tasks(), self.recovery_cleanup()])
    }

    fn image_tasks(&mut self) -> Task<Message> {
        self.images.request(
            &self.projection.rich.images,
            self.path.as_deref().and_then(std::path::Path::parent),
        )
    }

    fn update_message(&mut self, message: Message) -> Task<Message> {
        if self.loading
            && matches!(
                message,
                Message::ReplicaLoaded(_)
                    | Message::ProposalLoaded(_)
                    | Message::AgentUndo
                    | Message::AgentRedo
            )
        {
            self.notification =
                Some("Finish opening the document before importing changes.".into());
            return Task::none();
        }
        if self.loading
            && matches!(
                message,
                Message::Edit(_)
                    | Message::CommitComposition { .. }
                    | Message::Select(_)
                    | Message::SelectSource(_)
                    | Message::TableTab(_)
                    | Message::Format(_)
                    | Message::Undo
                    | Message::Redo
                    | Message::ToggleTask { .. }
                    | Message::ToggleTaskSource { .. }
                    | Message::ToggleCurrentTask
            )
        {
            return Task::none();
        }
        if self.overlay.is_some()
            && matches!(
                message,
                Message::Edit(_)
                    | Message::CommitComposition { .. }
                    | Message::Select(_)
                    | Message::SelectSource(_)
                    | Message::TableTab(_)
                    | Message::Activate(_)
                    | Message::JumpTo(_)
                    | Message::ToggleTask { .. }
                    | Message::ToggleTaskSource { .. }
                    | Message::ToggleCurrentTask
                    | Message::Append
                    | Message::Navigate(_)
                    | Message::Format(_)
                    | Message::Undo
                    | Message::Redo
                    | Message::New
                    | Message::Open
                    | Message::OpenPath(_)
                    | Message::Guide
                    | Message::Welcome
                    | Message::ToggleSource
                    | Message::ToggleReading
                    | Message::ToggleSearch
                    | Message::Replace
                    | Message::ReplaceAll
            )
        {
            return Task::none();
        }
        match message {
            Message::Outline(message) => return self.update_outline(message),
            Message::Writing(message) => return self.update_writing(message),
            Message::Recovery(message) => return self.update_recovery(message),
            Message::CommitComposition { target, text } => {
                if let Some(selection) = self.document.collaboration.resolve_composition(&target) {
                    self.selection = selection.clamp(self.document.source());
                    self.sync_editor();
                    self.document.break_history_group();
                    let task = self.edit(text_editor::Action::Edit(text_editor::Edit::Paste(
                        Arc::new(text),
                    )));
                    self.document.break_history_group();
                    return task;
                }
                self.notification =
                    Some("The composition belongs to an earlier document session.".into());
            }
            Message::OpenReplica => {
                self.overlay = None;
                return self.request(Pending::Replica);
            }
            Message::ReplicaChosen(path) => {
                if let Some(path) = path {
                    return Task::perform(
                        collaboration_store::read_replica(path),
                        Message::ReplicaOpened,
                    );
                }
                self.busy = false;
                self.loading = false;
            }
            Message::ReplicaOpened(result) => {
                self.busy = false;
                self.loading = false;
                match result.and_then(|state| {
                    Document::from_state(&state).map_err(|error| FileError::Io(error.to_string()))
                }) {
                    Ok(document) => {
                        self.install_document(document, None, "Offline copy.md".into());
                        self.dirty = true;
                        self.notification = Some("Compatible offline replica opened. Save a Markdown copy to keep its state.".into());
                    }
                    Err(error) => self.notification = Some(error.to_string()),
                }
            }
            Message::ShowCollaboration => self.overlay = Some(Overlay::Collaboration),
            Message::ExportReplica | Message::ExportAgentSnapshot => {
                let snapshot = self.document.collaboration.snapshot(self.document.source());
                let agent = matches!(message, Message::ExportAgentSnapshot);
                return Task::perform(
                    async move {
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .set_file_name(if agent {
                                "agent-snapshot.json"
                            } else {
                                "document.dostyreplica"
                            })
                            .save_file()
                            .await
                        else {
                            return Ok(());
                        };
                        if agent {
                            collaboration_store::write_snapshot(file.path().to_owned(), snapshot)
                                .await
                        } else {
                            collaboration_store::write_replica(
                                file.path().to_owned(),
                                snapshot.state,
                            )
                            .await
                        }
                    },
                    Message::CollaborationFinished,
                );
            }
            Message::ImportReplica => {
                return Task::perform(
                    async {
                        let file = rfd::AsyncFileDialog::new()
                            .add_filter(
                                "Dosty replica",
                                &["dostyreplica", "koanreplica", "daytripreplica"],
                            )
                            .pick_file()
                            .await
                            .ok_or_else(|| FileError::Io("Import cancelled".into()))?;
                        collaboration_store::read_replica(file.path().to_owned()).await
                    },
                    Message::ReplicaLoaded,
                );
            }
            Message::ImportProposal => {
                return Task::perform(
                    async {
                        let file = rfd::AsyncFileDialog::new()
                            .add_filter("Agent proposal", &["json"])
                            .pick_file()
                            .await
                            .ok_or_else(|| FileError::Io("Import cancelled".into()))?;
                        collaboration_store::read_proposal(file.path().to_owned()).await
                    },
                    Message::ProposalLoaded,
                );
            }
            Message::ReplicaLoaded(result) => {
                let selection = self.document.collaboration.selection(self.selection);
                match result {
                    Ok(state) => match self.document.receive(&state) {
                        Ok(()) => {
                            self.preserve_viewport_generation =
                                Some(self.document.collaboration.generation());
                            self.selection = self
                                .document
                                .collaboration
                                .resolve_selection(&selection)
                                .unwrap_or_default();
                            self.sync_editor();
                            self.changed();
                            self.notification = Some("Approved replica merged".into());
                        }
                        Err(error) => self.notification = Some(error.to_string()),
                    },
                    Err(error) => self.notification = Some(error.to_string()),
                }
            }
            Message::ProposalLoaded(result) => {
                let selection = self.document.collaboration.selection(self.selection);
                match result {
                    Ok(proposal) => match self.document.accept(&proposal) {
                        Ok(()) => {
                            self.preserve_viewport_generation =
                                Some(self.document.collaboration.generation());
                            self.selection = self
                                .document
                                .collaboration
                                .resolve_selection(&selection)
                                .unwrap_or_default();
                            self.sync_editor();
                            self.changed();
                            self.notification = Some(format!(
                                "Accepted proposal from {}",
                                self.document
                                    .collaboration
                                    .last_agent_author()
                                    .unwrap_or(&proposal.author)
                            ));
                        }
                        Err(error) => self.notification = Some(error.to_string()),
                    },
                    Err(error) => self.notification = Some(error.to_string()),
                }
            }
            Message::AgentUndo | Message::AgentRedo => {
                let selection = self.document.collaboration.selection(self.selection);
                if self
                    .document
                    .history(EditOrigin::Agent, matches!(message, Message::AgentRedo))
                    .unwrap_or_else(|error| {
                        self.notification = Some(error.to_string());
                        None
                    })
                    .is_some()
                {
                    self.preserve_viewport_generation =
                        Some(self.document.collaboration.generation());
                    self.selection = self
                        .document
                        .collaboration
                        .resolve_selection(&selection)
                        .unwrap_or_default();
                    self.sync_editor();
                    self.changed();
                }
            }
            Message::CollaborationFinished(result) => {
                self.notification = Some(match result {
                    Ok(()) => "Collaboration file exported".into(),
                    Err(error) => error.to_string(),
                })
            }

            Message::Edit(action) => return self.edit(action),
            Message::CopyCode(code) => {
                self.notification = Some("Code copied".into());
                return iced::clipboard::write(code);
            }
            Message::Select(selection) => {
                if self.mode == EditorMode::Live {
                    let selection = selection.clamp(&self.projection.text);
                    self.selection = self.projection.source_selection(selection, self.selection);
                    self.selection = self.projection.normalize_caret(self.selection);
                    self.editor_focused = true;
                    self.sync_editor();
                    self.document.break_history_group();
                }
            }
            Message::SelectSource(selection) => {
                if self.mode == EditorMode::Live {
                    self.selection = selection.clamp(self.document.source());
                    self.editor_focused = true;
                    self.sync_editor();
                    self.document.break_history_group();
                }
            }
            // Focus must reconcile even under a modal/loading guard. Otherwise
            // RichEditor emits the same mismatch on every redraw after a dialog
            // focuses its field, keeping Iced in an update/rebuild loop.
            Message::EditorFocused(focused) => {
                if self.editor_focused != focused {
                    self.editor_focused = focused;
                    self.sync_editor();
                }
            }
            Message::TableTab(backward) => return self.table_tab(backward),
            Message::ImageLoaded { key, result } => self.images.loaded(&key, result),
            Message::Activate(index) => return self.activate(index, false),
            Message::JumpTo(index) => {
                if self.mode == EditorMode::Read {
                    return viewport::reveal(index);
                }
                return self.activate(index, false);
            }
            Message::ToggleTask { block, task } => {
                let marker =
                    self.document.blocks.get(block).and_then(|block| {
                        self.projection.task_markers(block.range.clone()).nth(task)
                    });
                if let Some(marker) = marker {
                    self.toggle_task(marker);
                }
            }
            Message::ToggleTaskSource { marker, revision } => {
                // A queued pointer event must never edit another source revision.
                if Arc::ptr_eq(&revision, &self.projection.source_id) {
                    self.toggle_task(marker);
                }
            }
            Message::ToggleCurrentTask => {
                if self.mode != EditorMode::Read
                    && let Some(marker) = self.projection.task_marker_at(self.selection.head)
                {
                    self.toggle_task(marker);
                }
            }
            Message::Append => return self.append(),
            Message::Navigate(direction) => {
                let current = self.document.block_at(self.selection.head);
                let index = current
                    .saturating_add_signed(direction as isize)
                    .min(self.document.blocks.len() - 1);
                return self.activate(index, direction < 0);
            }
            Message::Format(format) => return self.format(format),
            Message::Undo | Message::Redo => {
                let selection = if matches!(message, Message::Undo) {
                    self.document.history(EditOrigin::Human, false)
                } else {
                    self.document.history(EditOrigin::Human, true)
                };
                let selection = match selection {
                    Ok(selection) => selection,
                    Err(error) => {
                        self.notification = Some(error.to_string());
                        return Task::none();
                    }
                };
                if let Some(selection) = selection {
                    if self.mode == EditorMode::Read {
                        self.mode = EditorMode::Live;
                    }
                    self.selection = selection;
                    self.sync_editor();
                    self.changed();
                    return operation::focus(EDITOR);
                }
            }
            Message::New => return self.request(Pending::New),
            Message::Open => return self.request(Pending::Open(None)),
            Message::Dropped(path) if crate::io::assets::is_image(&path) => {
                return self.import_dropped_image(path);
            }
            Message::OpenPath(path) | Message::Dropped(path) => {
                return self.request(Pending::Open(Some(path)));
            }
            Message::Guide => return self.request(Pending::Guide),
            Message::Welcome => return self.request(Pending::Welcome),
            Message::Save | Message::SaveAs | Message::ConfirmSave => {
                let save_as = matches!(message, Message::SaveAs);
                return self.save(save_as);
            }
            Message::ChosenFile(path) => {
                if let Some(path) = path {
                    return Task::perform(storage::load(path), Message::Loaded);
                }
                self.busy = false;
                self.loading = false;
            }
            Message::Loaded(result) => {
                self.busy = false;
                self.loading = false;
                match result {
                    Ok(file) => {
                        let version = file
                            .collaboration
                            .as_ref()
                            .map(collaboration_store::version);
                        let document = match file.collaboration {
                            Some(state) => match Document::from_state(&state) {
                                Ok(document) if document.source() == file.source => document,
                                Ok(_) => {
                                    self.notification =
                                        Some(FileError::MetadataConflict.to_string());
                                    return Task::none();
                                }
                                Err(error) => {
                                    self.notification = Some(error.to_string());
                                    return Task::none();
                                }
                            },
                            None => Document::new(file.source),
                        };
                        self.install_document(
                            document,
                            Some(file.path.clone()),
                            filename(&file.path),
                        );
                        self.saved_version = version;
                        self.remember(file.path);
                        return Task::batch([
                            self.persist_preferences(),
                            operation::snap_to(SCROLL, scrollable::RelativeOffset::START),
                        ]);
                    }
                    Err(error) => self.notification = Some(error.to_string()),
                }
            }
            Message::Saved {
                result,
                source,
                version,
                generation,
            } => {
                self.busy = false;
                match result {
                    Ok(path) => {
                        self.path = Some(path.clone());
                        self.display_name = filename(&path);
                        self.saved_source = (*source).clone();
                        self.saved_version = Some(version);
                        self.saved_generation = generation;
                        self.dirty = self.document.source() != self.saved_source
                            || self.document.collaboration.generation() != self.saved_generation;
                        self.remember(path);
                        if !self.dirty {
                            self.recovery.retire();
                        }
                        self.notification = Some("Document saved".into());
                        if !self.dirty
                            && let Some(pending) = self.pending.take()
                        {
                            self.overlay = None;
                            return Task::batch([
                                self.execute(pending),
                                self.persist_preferences(),
                            ]);
                        }
                        return Task::batch([
                            self.persist_preferences(),
                            Task::done(Message::Writing(writing::Message::ResumeImage)),
                        ]);
                    }
                    Err(FileError::Io(error)) if error == "Save cancelled" => {
                        self.writing.pending_media = None;
                    }
                    Err(error) => {
                        self.writing.pending_media = None;
                        self.notification = Some(error.to_string());
                    }
                }
            }
            Message::Export => {
                if self.busy {
                    return Task::none();
                }
                self.busy = true;
                let source = self.document.source().to_owned();
                let export_name = PathBuf::from(&self.display_name)
                    .with_extension("html")
                    .to_string_lossy()
                    .into_owned();
                let base = self
                    .path
                    .as_deref()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_owned());
                return Task::perform(
                    async move {
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .set_file_name(export_name)
                            .add_filter("HTML", &["html"])
                            .save_file()
                            .await
                        else {
                            return Ok(None);
                        };
                        let html = tokio::task::spawn_blocking(move || {
                            crate::io::export::html(&source, base.as_deref())
                        })
                        .await
                        .map_err(|e| FileError::Io(e.to_string()))??;
                        storage::save(file.path().to_path_buf(), html, None)
                            .await
                            .map(Some)
                    },
                    Message::Exported,
                );
            }
            Message::Exported(result) => {
                self.busy = false;
                self.notification = match result {
                    Ok(Some(_)) => Some("HTML exported".into()),
                    Ok(None) => None,
                    Err(error) => Some(error.to_string()),
                };
            }
            Message::ToggleSource => {
                self.mode = if self.mode == EditorMode::Source {
                    EditorMode::Live
                } else {
                    EditorMode::Source
                };
                self.document.break_history_group();
                self.sync_editor();
                return operation::focus(EDITOR);
            }
            Message::ToggleReading => {
                self.mode = if self.mode == EditorMode::Read {
                    EditorMode::Live
                } else {
                    EditorMode::Read
                };
                self.document.break_history_group();
                if self.mode == EditorMode::Live {
                    self.sync_editor();
                    return operation::focus(EDITOR);
                }
            }
            Message::ToggleSidebar => self.sidebar = !self.sidebar,
            Message::SidebarTab(tab) => self.sidebar_tab = tab,
            Message::ToggleFocus => self.focus = !self.focus,
            Message::ShowSettings => {
                self.document.break_history_group();
                self.overlay = Some(Overlay::Settings);
            }
            Message::CloseOverlay => {
                let quick_jump = matches!(self.overlay, Some(Overlay::QuickJump));
                drop(self.update_outline(outline::Message::CancelDrag));
                if !self.busy {
                    if self.overlay.is_none() {
                        if self.search_open {
                            self.search_open = false;
                        } else {
                            self.document.break_history_group();
                        }
                    }
                    self.overlay = None;
                    self.writing.close_details();
                    self.pending = None;
                    if quick_jump {
                        return operation::focus(EDITOR);
                    }
                }
            }
            Message::Theme(preset) => {
                self.resolved_theme = self
                    .preset_themes
                    .iter()
                    .find(|theme| theme.name == preset.name())
                    .cloned()
                    .unwrap_or_else(|| self.preset_themes[0].clone());
                self.preferences.theme = preset.name().into();
                self.preferences.custom_theme = None;
                return self.persist_preferences();
            }
            Message::LoadTheme => {
                return Task::perform(
                    async {
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .add_filter("Dosty theme", &["toml"])
                            .pick_file()
                            .await
                        else {
                            return Ok(None);
                        };
                        let source = tokio::fs::read_to_string(file.path()).await?;
                        if source.len() > 64 * 1024 {
                            return Err(FileError::Io(
                                "Theme files must be smaller than 64 KiB.".into(),
                            ));
                        }
                        Ok(Some((file.path().to_path_buf(), source)))
                    },
                    Message::ThemeLoaded,
                );
            }
            Message::ThemeLoaded(result) => match result {
                Ok(Some((path, source))) => match ResolvedTheme::from_toml(&source) {
                    Ok(theme) => {
                        self.preferences.theme = theme.name.clone();
                        self.preferences.custom_theme = Some(path);
                        self.resolved_theme = theme;
                        return self.persist_preferences();
                    }
                    Err(error) => self.notification = Some(error.to_string()),
                },
                Ok(None) => {}
                Err(error) => self.notification = Some(error.to_string()),
            },
            Message::PreferencesSaved(result) => {
                self.preferences_writing = false;
                if let Err(error) = result {
                    tracing::warn!(%error, "could not save preferences");
                    self.notification = Some(format!("Could not save preferences: {error}"));
                }
                if self.preferences_pending {
                    self.preferences_pending = false;
                    return self.persist_preferences();
                }
            }
            Message::ToggleSearch => {
                self.search_open = !self.search_open;
                if self.search_open {
                    return operation::focus(SEARCH);
                }
            }
            Message::Query(query) => {
                self.query = query;
                self.match_revealed = false;
                self.match_index = 0;
                self.refresh_matches();
            }
            Message::Replacement(replacement) => self.replacement = replacement,
            Message::FindNext | Message::FindPrevious => {
                if self.matches.is_empty() {
                    return Task::none();
                }
                if !self.match_revealed {
                    self.match_index = 0;
                } else if matches!(message, Message::FindPrevious) {
                    self.match_index =
                        (self.match_index + self.matches.len() - 1) % self.matches.len();
                } else {
                    self.match_index = (self.match_index + 1) % self.matches.len();
                }
                self.match_revealed = true;
                return self.reveal_match();
            }
            Message::Replace => {
                if let Some(range) = self.matches.get(self.match_index).cloned() {
                    if self.document.source().len() - range.len() + self.replacement.len()
                        > storage::MAX_FILE_BYTES as usize
                    {
                        self.notification =
                            Some("Replacement would exceed the 16 MiB document limit.".into());
                        return Task::none();
                    }
                    self.document.break_history_group();
                    self.document
                        .replace(range, &self.replacement)
                        .unwrap_or_else(|error| self.edit_error = Some(error.to_string()));
                    self.sync_editor();
                    self.changed();
                    self.sync_editor();
                    return self.reveal_match();
                }
            }
            Message::ReplaceAll => {
                if self.query.is_empty() {
                    return Task::none();
                }
                let count = self.document.source().matches(&self.query).count();
                let projected = self.document.source().len() as u128
                    + count as u128 * self.replacement.len() as u128;
                let removed = count as u128 * self.query.len() as u128;
                if projected.saturating_sub(removed) > u128::from(storage::MAX_FILE_BYTES) {
                    self.notification =
                        Some("Replacement would exceed the 16 MiB document limit.".into());
                    return Task::none();
                }
                let selection = self.document.collaboration.selection(self.selection);
                let count = self
                    .document
                    .replace_all(&self.query, &self.replacement)
                    .unwrap_or_else(|error| {
                        self.edit_error = Some(error.to_string());
                        0
                    });
                self.selection = self
                    .document
                    .collaboration
                    .resolve_selection(&selection)
                    .unwrap_or_default();
                self.preserve_viewport_generation = Some(self.document.collaboration.generation());
                self.active = None;
                let failed = self.edit_error.is_some();
                self.changed();
                self.sync_editor();
                if failed {
                    return Task::none();
                }
                self.notification = Some(format!(
                    "Replaced {count} occurrence{}",
                    if count == 1 { "" } else { "s" }
                ));
            }
            Message::ConfirmDiscard => {
                if !self.busy
                    && let Some(pending) = self.pending.take()
                {
                    self.overlay = None;
                    return self.execute(pending);
                }
            }
            Message::CloseRequested(id) => return self.request(Pending::Close(id)),
            Message::Link(url) => return self.open_destination(url),
            Message::LinkOpened(result) => {
                if let Err(error) = result {
                    self.notification = Some(error);
                }
            }
            Message::Dismiss => self.notification = None,
            Message::Zoom(delta) => self.zoom = (self.zoom + delta).clamp(0.75, 1.5),
        }
        Task::none()
    }

    fn commit_local(
        &mut self,
        range: Range<usize>,
        text: &str,
        before: Selection,
        after: Selection,
    ) {
        if let Err(error) = self.document.replace_edit(range, text, before, after) {
            self.edit_error = Some(error.to_string());
            self.edit_rollback = Some(before);
        }
    }

    fn changed(&mut self) {
        self.recovery.edited();
        self.dirty = self.document.source() != self.saved_source
            || self.document.collaboration.generation() != self.saved_generation;
        self.notification = self.edit_error.take();
        self.refresh_matches();
    }

    fn refresh_matches(&mut self) {
        self.matches = self.document.matches(&self.query);
        self.match_index = self.match_index.min(self.matches.len().saturating_sub(1));
    }

    fn edit(&mut self, action: text_editor::Action) -> Task<Message> {
        if self.mode == EditorMode::Read {
            return Task::none();
        }
        if !self.editor_focused {
            self.editor_focused = true;
            self.sync_editor();
        }
        let before = self.selection;
        if let text_editor::Action::Edit(edit) = &action
            && let Some(edit) = (self.mode == EditorMode::Live)
                .then(|| {
                    crate::document::code_edit::insert_at_fence(
                        self.document.source(),
                        &self.projection.rich,
                        before,
                        edit,
                    )
                })
                .flatten()
                .or_else(|| {
                    crate::document::code_edit::edit(
                        self.document.source(),
                        &self.projection.rich,
                        before,
                        edit,
                    )
                })
        {
            if self.document.source().len() - edit.range.len() + edit.text.len()
                <= storage::MAX_FILE_BYTES as usize
            {
                self.document.break_history_group();
                self.commit_local(edit.range, &edit.text, before, edit.selection);
                self.selection = edit.selection;
                self.sync_editor();
                self.changed();
                self.document.break_history_group();
            } else {
                self.notification = Some("The editor supports documents up to 16 MiB.".into());
            }
            return Task::none();
        }
        if self.mode == EditorMode::Live
            && let Some((table, row, column)) = table_edit::cell_at(&self.projection, before)
        {
            let cell = &table.rows[row][column];
            let display = self.projection.display_selection(before);
            if matches!(action, text_editor::Action::Edit(text_editor::Edit::Enter)) {
                return self.table_move(false, true);
            }
            if matches!(&action, text_editor::Action::Edit(text_editor::Edit::Paste(text)) if text.as_str() == "\n")
            {
                let end = table.source_range.end;
                let text = if self.document.source()[..end].ends_with('\n') {
                    "\n"
                } else {
                    "\n\n"
                };
                self.apply_table_edit(projection::ProjectedEdit {
                    range: end..end,
                    text: text.into(),
                    selection: Selection::caret(end + text.len()),
                });
                return Task::none();
            }
            if before.is_caret() {
                let backward = matches!(
                    action,
                    text_editor::Action::Edit(text_editor::Edit::Backspace)
                ) && display.head == cell.start;
                let forward =
                    matches!(action, text_editor::Action::Edit(text_editor::Edit::Delete))
                        && display.head == cell.end;
                if backward || forward {
                    let cells = table.rows.iter().flatten().collect::<Vec<_>>();
                    let index = cells.iter().position(|range| *range == cell).unwrap_or(0);
                    let point = if backward {
                        index
                            .checked_sub(1)
                            .and_then(|index| cells.get(index))
                            .map_or(cell.start.saturating_sub(1), |cell| cell.end)
                    } else {
                        cells
                            .get(index + 1)
                            .map_or((cell.end + 1).min(self.projection.text.len()), |cell| {
                                cell.start
                            })
                    };
                    return self.update_message(Message::Select(Selection::caret(point)));
                }
            }
            let text = match &action {
                text_editor::Action::Edit(text_editor::Edit::Insert('|'))
                    if self.document.source()[..before.head]
                        .bytes()
                        .rev()
                        .take_while(|&b| b == b'\\')
                        .count()
                        % 2
                        == 0 =>
                {
                    Some("|")
                }
                text_editor::Action::Edit(text_editor::Edit::Paste(text))
                    if text.contains(['|', '\n', '\r', '\t']) =>
                {
                    Some(text.as_str())
                }
                _ => None,
            };
            if let Some(text) = text {
                if let Some(edit) =
                    table_edit::paste(&self.projection, self.document.source(), before, text)
                {
                    self.apply_table_edit(edit);
                }
                return Task::none();
            }
        }
        let edited = action.is_edit();
        if self.mode == EditorMode::Live && before.is_caret() {
            let source = self.document.source();
            let line_start = source[..before.head].rfind('\n').map_or(0, |i| i + 1);
            let line_end = source[before.head..]
                .find('\n')
                .map_or(source.len(), |i| before.head + i);
            let in_code = self.projection.rich.source_code_at(before.head).is_some();
            if !in_code && let Some(prefix) = block_prefix(&source[line_start..line_end]) {
                let display = self.projection.to_display(before.head);
                if matches!(
                    action,
                    text_editor::Action::Edit(text_editor::Edit::Backspace)
                ) && display == self.projection.to_display(line_start + prefix.length)
                    && self.projection.to_display(line_start)
                        == self.projection.to_display(line_start + prefix.length)
                {
                    self.document.break_history_group();
                    self.selection = Selection::caret(line_start);
                    self.commit_local(
                        line_start..line_start + prefix.length,
                        "",
                        before,
                        self.selection,
                    );
                    self.sync_editor();
                    self.changed();
                    return Task::none();
                }
                if matches!(action, text_editor::Action::Edit(text_editor::Edit::Enter))
                    && !prefix.continuation.is_empty()
                {
                    let empty = source[line_start + prefix.length..line_end]
                        .trim()
                        .is_empty();
                    self.document.break_history_group();
                    let range = if empty {
                        line_start..before.head
                    } else {
                        before.head..before.head
                    };
                    let replacement = if empty {
                        "\n".into()
                    } else {
                        format!("\n{}", prefix.continuation)
                    };
                    self.selection = Selection::caret(range.start + replacement.len());
                    self.commit_local(range, &replacement, before, self.selection);
                    self.sync_editor();
                    self.changed();
                    self.document.break_history_group();
                    return Task::none();
                }
            }
        }
        let select_all = matches!(action, text_editor::Action::SelectAll);
        let document_motion = match &action {
            text_editor::Action::Move(motion) | text_editor::Action::Select(motion) => {
                let head = match motion {
                    text_editor::Motion::DocumentStart => Some(0),
                    text_editor::Motion::DocumentEnd => Some(self.document.source().len()),
                    _ => None,
                };
                head.map(|head| Selection {
                    anchor: if matches!(action, text_editor::Action::Select(_)) {
                        before.anchor
                    } else {
                        head
                    },
                    head,
                })
            }
            _ => None,
        };
        let enter = matches!(action, text_editor::Action::Edit(text_editor::Edit::Enter));
        let changed = self.editor.perform(action);
        self.native_edited = edited;
        let after = self.editor.snapshot();
        let display_selection = self.editor.selection();
        if !edited || (!changed && before.is_caret()) {
            if select_all {
                self.selection = Selection {
                    anchor: 0,
                    head: self.document.source().len(),
                };
            } else if let Some(selection) = document_motion {
                self.selection = selection;
            } else if self.mode == EditorMode::Source {
                self.selection = display_selection;
            } else {
                self.selection = self.projection.source_selection(display_selection, before);
            }
            self.active = Some(self.document.block_at(self.selection.head));
            if !edited {
                self.document.break_history_group();
            }
            self.sync_editor();
            return Task::none();
        }
        let edit = if self.mode == EditorMode::Source {
            let diff = projection::difference(self.document.source(), after);
            projection::ProjectedEdit {
                range: diff.old,
                text: after[diff.new].to_owned(),
                selection: display_selection,
            }
        } else {
            self.projection
                .edit(self.document.source(), after, before, display_selection)
        };
        if self.document.source().len() - edit.range.len() + edit.text.len()
            > storage::MAX_FILE_BYTES as usize
        {
            self.notification = Some("The editor supports documents up to 16 MiB.".into());
            self.sync_editor();
            return Task::none();
        }
        self.commit_local(edit.range, &edit.text, before, edit.selection);
        self.selection = edit.selection;
        self.sync_editor();
        self.changed();
        if enter {
            self.document.break_history_group();
        }
        Task::none()
    }

    /// Updates the existing native buffer with a minimal splice, keeping its
    /// widget state (including drag selection and IME preedit) alive.
    fn sync_editor(&mut self) {
        self.sync_outline();
        if let Some(selection) = self.edit_rollback.take() {
            self.selection = selection;
        }
        self.selection = self.selection.clamp(self.document.source());
        let caret =
            (self.mode == EditorMode::Live && self.editor_focused).then_some(self.selection.head);
        let previous_highlights = Arc::clone(&self.projection.highlights);
        if self.projection_revision != Some(self.document.revision) {
            let previous_revision = self.projection_revision;
            self.projection_revision = Some(self.document.revision);
            let updated = self.projection.update_words(self.document.source(), caret)
                || (self
                    .document
                    .block_changes
                    .first()
                    .is_some_and(|change| Some(change.previous_revision) == previous_revision)
                    && self.projection.update_block_batch(
                        self.document.source(),
                        &self.document.block_changes,
                        caret,
                    ));
            if !updated {
                self.projection = Projection::with_context(self.document.source(), caret);
            }
        } else {
            let disclosure = self.projection.disclosure_at(caret);
            if self.projection.disclosure != disclosure {
                self.projection.set_disclosure(disclosure);
            }
        }
        let native_edited = std::mem::take(&mut self.native_edited);
        let current = self.editor.snapshot();
        let target = if self.mode == EditorMode::Source {
            self.document.source()
        } else {
            &self.projection.text
        };
        let selection = if self.mode == EditorMode::Source {
            self.selection
        } else {
            self.projection.display_selection(self.selection)
        };
        if current != target {
            // Distant disclosure can change two small spans with megabytes of
            // unchanged text between them. Replacing that envelope is quadratic
            // in cosmic-text's line deletion. Apply proven regions from the end
            // so earlier native line/column positions remain valid.
            let regions = self.projection.change.as_ref().filter(|change| {
                !native_edited
                    && !self.editor_source
                    && self.mode != EditorMode::Source
                    && Arc::ptr_eq(&previous_highlights, &change.previous)
            });
            let changes = regions.map_or_else(
                || native_buffer::synchronization_changes(current, target),
                |change| {
                    change
                        .regions
                        .iter()
                        .map(|region| {
                            let diff = projection::difference(
                                &current[region.old.clone()],
                                &target[region.new.clone()],
                            );
                            projection::TextChange {
                                old: region.old.start + diff.old.start
                                    ..region.old.start + diff.old.end,
                                new: region.new.start + diff.new.start
                                    ..region.new.start + diff.new.end,
                            }
                        })
                        .collect()
                },
            );
            for diff in changes.into_iter().rev() {
                if diff.old.is_empty() && diff.new.is_empty() {
                    continue;
                }
                self.editor.replace(diff.old, &target[diff.new]);
            }
        }
        if self.editor.selection() != selection
            || selection.is_caret() && self.editor.content().cursor().selection.is_some()
        {
            self.editor.select(selection);
        }
        self.editor_source = self.mode == EditorMode::Source;
        self.active = Some(self.document.block_at(self.selection.head));
    }

    fn apply_table_edit(&mut self, edit: projection::ProjectedEdit) {
        if self.document.source().len() - edit.range.len() + edit.text.len()
            > storage::MAX_FILE_BYTES as usize
        {
            self.notification = Some("The editor supports documents up to 16 MiB.".into());
            return;
        }
        self.document.break_history_group();
        let before = self.selection;
        self.selection = edit.selection;
        self.commit_local(edit.range, &edit.text, before, self.selection);
        self.sync_editor();
        self.changed();
        self.document.break_history_group();
    }

    fn toggle_task(&mut self, marker: Range<usize>) {
        if self.projection.task_marker_at(marker.start) != Some(marker.clone()) {
            return;
        }
        let replacement = match self.document.source().get(marker.clone()) {
            Some("[ ]") => "[x]",
            Some("[x]" | "[X]") => "[ ]",
            _ => return,
        };
        // Both forms have the same byte length. Keep both source endpoints,
        // focus, and widget wrap intent; save the same selection in history.
        let selection = self.selection;
        self.document.break_history_group();
        self.commit_local(marker, replacement, selection, selection);
        self.preserve_viewport_generation = Some(self.document.collaboration.generation());
        self.sync_editor();
        self.changed();
        self.document.break_history_group();
    }

    fn table_tab(&mut self, backward: bool) -> Task<Message> {
        self.table_move(backward, false)
    }

    fn table_move(&mut self, backward: bool, next_row: bool) -> Task<Message> {
        let Some((table, row, column)) = table_edit::cell_at(&self.projection, self.selection)
        else {
            return Task::none();
        };
        let target = if next_row {
            table.rows.get(row + 1).and_then(|row| row.get(column))
        } else {
            let cells = table.rows.iter().flatten().collect::<Vec<_>>();
            let index = cells
                .iter()
                .position(|cell| **cell == table.rows[row][column])
                .unwrap_or(0);
            if backward {
                index
                    .checked_sub(1)
                    .and_then(|index| cells.get(index).copied())
            } else {
                cells.get(index + 1).copied()
            }
        };
        if let Some(cell) = target {
            let point = cell.start;
            return self.update_message(Message::Select(Selection::caret(point)));
        }
        if !backward {
            let end = table.source_range.end;
            let prefix = table_edit::row_prefix(self.document.source(), table);
            let newline = if self.document.source()[..end].ends_with('\n') {
                ""
            } else {
                "\n"
            };
            let text = format!(
                "{newline}{prefix}|{}\n",
                "  |".repeat(table.alignment.len())
            );
            let point =
                end + newline.len() + prefix.len() + 3 + if next_row { column * 3 } else { 0 };
            self.apply_table_edit(projection::ProjectedEdit {
                range: end..end,
                text,
                selection: Selection::caret(point),
            });
        }
        Task::none()
    }

    fn activate(&mut self, index: usize, at_end: bool) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        let Some(block) = self.document.blocks.get(index) else {
            return Task::none();
        };
        let offset = if at_end {
            block.range.end
        } else {
            block.range.start
        };
        if self.mode == EditorMode::Read {
            self.mode = EditorMode::Live;
        }
        self.selection = Selection::caret(offset);
        self.editor_focused = true;
        self.document.break_history_group();
        self.sync_editor();
        operation::focus(EDITOR)
    }

    fn append(&mut self) -> Task<Message> {
        self.mode = EditorMode::Live;
        self.editor_focused = true;
        self.selection = Selection::caret(self.document.source().len());
        self.sync_editor();
        operation::focus(EDITOR)
    }

    fn format(&mut self, format: Format) -> Task<Message> {
        if matches!(format, Format::Link) {
            return self.update_writing(writing::Message::Details(writing::Kind::Link));
        }
        if self.mode == EditorMode::Read {
            self.mode = EditorMode::Live;
            self.sync_editor();
        }
        let selected = self.editor.content().selection().unwrap_or_default();
        if self.mode == EditorMode::Live && !selected.is_empty() {
            let marker = match format {
                Format::Bold => Some("**"),
                Format::Italic => Some("*"),
                Format::Strikethrough => Some("~~"),
                Format::Code => Some("`"),
                _ => None,
            };
            let concealed = Projection::new(self.document.source());
            if let Some(marker) = marker
                && let Some(edit) = concealed.format_inline(
                    self.document.source(),
                    concealed.display_selection(self.selection),
                    marker,
                )
            {
                if self.document.source().len() - edit.range.len() + edit.text.len()
                    > storage::MAX_FILE_BYTES as usize
                {
                    self.notification =
                        Some("Formatting would exceed the 16 MiB document limit.".into());
                    return Task::none();
                }
                self.document.break_history_group();
                self.commit_local(edit.range, &edit.text, self.selection, edit.selection);
                self.selection = edit.selection;
                self.sync_editor();
                self.changed();
                self.document.break_history_group();
                return operation::focus(EDITOR);
            }
        }
        let replacement = match format {
            Format::Bold => format!(
                "**{}**",
                if selected.is_empty() {
                    "bold text"
                } else {
                    &selected
                }
            ),
            Format::Italic => format!(
                "*{}*",
                if selected.is_empty() {
                    "italic text"
                } else {
                    &selected
                }
            ),
            Format::Code => format!(
                "`{}`",
                if selected.is_empty() {
                    "code"
                } else {
                    &selected
                }
            ),
            Format::Strikethrough => format!(
                "~~{}~~",
                if selected.is_empty() {
                    "deleted text"
                } else {
                    &selected
                }
            ),
            Format::Heading => "## ".into(),
            Format::Quote => "> ".into(),
            Format::List => "- ".into(),
            Format::Task => "- [ ] ".into(),
            Format::Link => format!(
                "[{}](https://example.com)",
                if selected.is_empty() {
                    "link text"
                } else {
                    &selected
                }
            ),
            Format::Fence => format!("\n```rust\n{selected}\n```\n"),
            Format::Table => "\n| Column | Column |\n| --- | --- |\n| Text | Text |\n".into(),
        };
        self.document.break_history_group();
        if matches!(
            format,
            Format::Heading | Format::Quote | Format::List | Format::Task
        ) {
            // Insert block prefixes in source space; display line starts may be
            // after a concealed heading or quote marker.
            let line_start = self.document.source()[..self.selection.head]
                .rfind('\n')
                .map_or(0, |index| index + 1);
            let before = self.selection;
            if self.document.source().len() + replacement.len() > storage::MAX_FILE_BYTES as usize {
                self.notification =
                    Some("Formatting would exceed the 16 MiB document limit.".into());
                return Task::none();
            }
            self.commit_local(
                line_start..line_start,
                &replacement,
                before,
                Selection::caret(before.head + replacement.len()),
            );
            self.selection = Selection::caret(before.head + replacement.len());
            self.sync_editor();
            self.changed();
            self.document.break_history_group();
            return operation::focus(EDITOR);
        }
        let task = self.edit(text_editor::Action::Edit(text_editor::Edit::Paste(
            Arc::new(replacement),
        )));
        self.document.break_history_group();
        Task::batch([task, operation::focus(EDITOR)])
    }

    fn reveal_match(&mut self) -> Task<Message> {
        let Some(range) = self.matches.get(self.match_index).cloned() else {
            return Task::none();
        };
        if self.mode == EditorMode::Read {
            self.mode = EditorMode::Live;
        }
        self.selection = Selection {
            anchor: range.start,
            head: range.end,
        };
        // Source-only matches (e.g. a link destination) must remain inspectable.
        if self.mode == EditorMode::Live
            && self.projection.to_display(range.start) == self.projection.to_display(range.end)
        {
            self.mode = EditorMode::Source;
        }
        self.sync_editor();
        operation::focus(EDITOR)
    }

    fn request(&mut self, pending: Pending) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        if self.dirty {
            self.pending = Some(pending);
            self.overlay = Some(Overlay::Unsaved);
            Task::none()
        } else {
            self.execute(pending)
        }
    }

    fn execute(&mut self, pending: Pending) -> Task<Message> {
        match pending {
            Pending::Recover(snapshot) => self.restore_draft(*snapshot),
            Pending::Sample => {
                self.busy = true;
                self.loading = true;
                return Task::perform(crate::io::writing_sample::load(), Message::Loaded);
            }
            Pending::New => {
                self.set_document(String::new(), None, "Untitled.md".into());
                return self.activate(0, false);
            }
            Pending::Guide => self.set_document(
                include_str!("../../assets/guide.md").into(),
                None,
                "Markdown guide.md".into(),
            ),
            Pending::Welcome => self.set_document(
                include_str!("../../assets/welcome.md").into(),
                None,
                "Welcome.md".into(),
            ),
            Pending::Open(path) => {
                self.busy = true;
                self.loading = true;
                return if let Some(path) = path {
                    Task::perform(storage::load(path), Message::Loaded)
                } else {
                    Task::perform(
                        async {
                            rfd::AsyncFileDialog::new()
                                .add_filter("Markdown & text", &["md", "markdown", "txt"])
                                .pick_file()
                                .await
                                .map(|file| file.path().to_path_buf())
                        },
                        Message::ChosenFile,
                    )
                };
            }
            Pending::Replica => {
                self.busy = true;
                self.loading = true;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter(
                                "Dosty replica",
                                &["dostyreplica", "koanreplica", "daytripreplica"],
                            )
                            .pick_file()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    Message::ReplicaChosen,
                );
            }
            Pending::Close(id) => {
                self.recovery.retire();
                return window::close(id);
            }
        }
        operation::snap_to(SCROLL, scrollable::RelativeOffset::START)
    }

    fn set_document(&mut self, source: String, path: Option<PathBuf>, name: String) {
        self.install_document(Document::new(source), path, name);
    }

    fn install_document(&mut self, document: Document, path: Option<PathBuf>, name: String) {
        self.recovery.retire();
        self.writing = writing::State::default();
        self.outline = outline::State::default();
        self.images.clear_history();
        self.overlay = None;
        self.document = document;
        self.projection = Projection::new(self.document.source());
        self.projection_revision = Some(self.document.revision);
        self.editor_source = self.mode == EditorMode::Source;
        self.editor = native_buffer::Buffer::new(if self.editor_source {
            self.document.source()
        } else {
            &self.projection.text
        });
        self.native_edited = false;
        self.edit_error = None;
        self.edit_rollback = None;
        self.preserve_viewport_generation = None;
        self.editor_focused = false;
        self.saved_source = self.document.source().to_owned();
        self.saved_version = None;
        self.saved_generation = 0;
        self.path = path;
        self.display_name = name;
        self.dirty = false;
        self.selection = Selection::default();
        self.active = None;
        self.notification = None;
        self.refresh_matches();
        self.sync_editor();
        self.selection = self.projection.normalize_caret(self.selection);
    }

    fn save(&mut self, save_as: bool) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        self.busy = true;
        let path = if save_as { None } else { self.path.clone() };
        let expected = path.as_ref().map(|_| self.saved_source.clone());
        let source = Arc::new(self.document.source().to_owned());
        let result_source = Arc::clone(&source);
        let state = self.document.collaboration.state();
        let version = collaboration_store::version(&state);
        let generation = self.document.collaboration.generation();
        let expected_version = if save_as {
            None
        } else {
            self.saved_version.clone()
        };
        let name = self.display_name.clone();
        let old_path = self.path.clone();
        let image_history = self.images.history();
        Task::perform(
            async move {
                let path = if let Some(path) = path {
                    path
                } else {
                    rfd::AsyncFileDialog::new()
                        .set_file_name(name)
                        .add_filter("Markdown", &["md"])
                        .save_file()
                        .await
                        .ok_or_else(|| FileError::Io("Save cancelled".into()))?
                        .path()
                        .to_path_buf()
                };
                let asset_source = Arc::clone(&source);
                let asset_path = path.clone();
                tokio::task::spawn_blocking(move || {
                    crate::io::assets::relocate(
                        &asset_source,
                        old_path.as_deref(),
                        &asset_path,
                        &image_history,
                    )
                })
                .await
                .map_err(|error| FileError::Io(error.to_string()))??;
                collaboration_store::save_document(
                    path,
                    (*source).clone(),
                    state,
                    expected,
                    expected_version,
                )
                .await
            },
            move |result| Message::Saved {
                result,
                source: Arc::clone(&result_source),
                version: version.clone(),
                generation,
            },
        )
    }

    fn remember(&mut self, path: PathBuf) {
        self.preferences.recent.retain(|existing| existing != &path);
        self.preferences.recent.insert(0, path);
        self.preferences.recent.truncate(8);
    }

    fn persist_preferences(&mut self) -> Task<Message> {
        if self.preferences_writing {
            self.preferences_pending = true;
            return Task::none();
        }
        self.preferences_writing = true;
        Task::perform(
            storage::save_preferences(self.preferences.clone()),
            Message::PreferencesSaved,
        )
    }

    pub fn view(&self) -> Element<'_, Message> {
        let tokens = self.resolved_theme.tokens;
        let content = column![
            self.header(),
            self.search_bar(),
            self.writing_surface(),
            self.status_bar()
        ]
        .spacing(0)
        .height(Fill);
        let base = row![
            if self.sidebar && !self.focus {
                self.sidebar_view()
            } else {
                space().width(0).into()
            },
            content
        ]
        .height(Fill);
        let mut layers = stack![base];
        if let Some(overlay) = self.overlay {
            let backdrop = mouse_area(
                container(space())
                    .width(Fill)
                    .height(Fill)
                    .style(styles::panel(tokens.scrim)),
            )
            .on_press(Message::CloseOverlay);
            let modal = responsive(move |size| {
                let modal = match overlay {
                    Overlay::Settings => self.settings_view((size.height - 48.0).min(760.0)),
                    Overlay::Collaboration => self.collaboration_view(),
                    Overlay::Unsaved => self.unsaved_view(),
                    Overlay::Writing => self.writing_view(),
                    Overlay::Recovery => self.recovery_view(),
                    Overlay::QuickJump => self.quick_jump_view(),
                };
                container(
                    scrollable(container(modal).padding(16))
                        .height(Shrink)
                        .style(styles::scrollbar(tokens)),
                )
                .max_height(size.height)
                .center_x(Fill)
                .center_y(Fill)
                .padding(8)
                .into()
            });
            layers = layers.push(backdrop).push(modal);
        }
        if let Some(notification) = &self.notification {
            let toast = button(
                row![
                    text(notification).size(13).color(tokens.text),
                    icon::view(Icon::Close, tokens.muted)
                ]
                .spacing(18)
                .align_y(Alignment::Center),
            )
            .on_press(Message::Dismiss)
            .padding([12, 16])
            .style(styles::button_style(tokens, Button::Selected));
            layers = layers.push(
                container(toast)
                    .align_bottom(Fill)
                    .center_x(Fill)
                    .padding(48),
            );
        }
        layers.into()
    }

    fn header(&self) -> Element<'_, Message> {
        let tokens = self.resolved_theme.tokens;
        let header = row![
            if self.focus {
                space().width(0).into()
            } else {
                self.icon_button(
                    Icon::Sidebar,
                    "Toggle sidebar",
                    Message::ToggleSidebar,
                    self.sidebar,
                )
            },
            container(
                text(self.display_name.trim_end_matches(".md"))
                    .size(13)
                    .font(Font {
                        weight: iced::font::Weight::Medium,
                        ..Font::DEFAULT
                    })
                    .color(tokens.text)
                    .wrapping(text::Wrapping::None)
                    .width(Fill)
            )
            .width(Fill)
            .clip(true),
            text(if self.dirty { "●" } else { "" })
                .size(8)
                .color(tokens.accent),
            self.icon_button(
                Icon::Search,
                "Find · ⌘/Ctrl F",
                Message::ToggleSearch,
                self.search_open
            ),
            self.icon_button(
                Icon::Focus,
                "Focus mode · ⌘/Ctrl Shift F",
                Message::ToggleFocus,
                self.focus
            ),
            self.icon_button(Icon::Export, "Export HTML", Message::Export, false),
            self.icon_button(Icon::Save, "Save · ⌘/Ctrl S", Message::Save, false),
        ]
        .spacing(9)
        .align_y(Alignment::Center);
        container(header).padding([14, 22]).height(62).into()
    }

    fn sidebar_view(&self) -> Element<'_, Message> {
        let tokens = self.resolved_theme.tokens;
        let brand = row![
            brand::view(tokens),
            space::horizontal(),
            self.icon_button(Icon::Plus, "New document · ⌘/Ctrl N", Message::New, false)
        ]
        .align_y(Alignment::Center);
        let tabs = row![
            self.small_button(
                "Files",
                Message::SidebarTab(SidebarTab::Files),
                self.sidebar_tab == SidebarTab::Files
            ),
            self.small_button(
                "Outline",
                Message::SidebarTab(SidebarTab::Outline),
                self.sidebar_tab == SidebarTab::Outline
            )
        ]
        .spacing(6);
        let mut list = column![].spacing(4);
        if self.sidebar_tab == SidebarTab::Outline {
            list = list.push(self.outline_list());
        } else {
            list = list.push(text("WORKSPACE").size(10).color(tokens.faint));
            list = list.push(self.file_button(&self.display_name, None, true));
            list = list.push(self.file_action(
                "Recovery copies",
                Message::Recovery(recovery::Message::Show),
                false,
            ));
            list = list.push(self.file_action(
                "The quiet hour · sample",
                Message::Writing(writing::Message::Sample),
                false,
            ));
            if self.display_name != "Welcome.md" {
                list = list.push(self.file_action("Welcome", Message::Welcome, false));
            }
            if self.display_name != "Markdown guide.md" {
                list = list.push(self.file_action("Markdown guide", Message::Guide, false));
            }
            if self
                .preferences
                .recent
                .iter()
                .any(|path| Some(path) != self.path.as_ref())
            {
                list = list
                    .push(space().height(22))
                    .push(text("RECENT").size(10).color(tokens.faint))
                    .push(space().height(5));
                for path in &self.preferences.recent {
                    if Some(path) != self.path.as_ref() {
                        list =
                            list.push(self.file_button(&filename(path), Some(path.clone()), false));
                    }
                }
            }
        }
        let bottom = column![
            rule::horizontal(1).style(move |_| rule::Style {
                color: tokens.border,
                snap: true,
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full
            }),
            self.action_row(Icon::Folder, "Open a document", Message::Open),
            self.action_row(
                Icon::Settings,
                "Appearance & shortcuts",
                Message::ShowSettings
            ),
        ]
        .spacing(8);
        container(
            column![
                brand,
                space().height(16),
                tabs,
                space().height(12),
                scrollable(list)
                    .height(Fill)
                    .style(styles::scrollbar(tokens)),
                bottom
            ]
            .spacing(12),
        )
        .padding(20)
        .width(260)
        .height(Fill)
        .style(styles::panel(tokens.surface))
        .into()
    }

    fn writing_surface(&self) -> Element<'_, Message> {
        let tokens = self.resolved_theme.tokens;
        let width = self.resolved_theme.typography.content_width;
        if self.mode != EditorMode::Read {
            return container(
                column![
                    if self.focus {
                        space().height(8).into()
                    } else {
                        self.format_bar()
                    },
                    space().height(if self.focus { 16 } else { 24 }),
                    self.editor_view(self.mode == EditorMode::Source)
                ]
                .spacing(0)
                .max_width(width)
                .height(Fill),
            )
            .padding(iced::Padding::new(32.0).top(24.0).bottom(8.0))
            .center_x(Fill)
            .height(Fill)
            .into();
        }
        let mut blocks = column![].spacing(12);
        let mut settings = styles::markdown_settings(&self.resolved_theme);
        settings.text_size = (settings.text_size.0 * self.zoom).into();
        settings.h1_size = (settings.h1_size.0 * self.zoom).into();
        settings.h2_size = (settings.h2_size.0 * self.zoom).into();
        settings.h3_size = (settings.h3_size.0 * self.zoom).into();
        settings.h4_size = (settings.h4_size.0 * self.zoom).into();
        settings.h5_size = (settings.h5_size.0 * self.zoom).into();
        settings.h6_size = (settings.h6_size.0 * self.zoom).into();
        settings.code_size = (settings.code_size.0 * self.zoom).into();
        for (index, _) in self.document.blocks.iter().enumerate() {
            let items = self.document.rendered(index);
            let rendered: Element<'_, Message> = if items.is_empty() {
                text(self.document.block_source(index))
                    .font(styles::BODY_FONT)
                    .size(18)
                    .color(tokens.muted)
                    .into()
            } else {
                markdown::view_with(
                    items,
                    settings,
                    &MarkdownView {
                        tokens,
                        block: index,
                        base: self
                            .path
                            .as_deref()
                            .and_then(std::path::Path::parent)
                            .map(std::path::Path::to_path_buf),
                        line_height: self.resolved_theme.typography.line_height,
                        task: std::cell::Cell::new(0),
                    },
                )
            };
            blocks = blocks.push(
                container(
                    mouse_area(container(rendered).padding([5, 12]).width(Fill))
                        .on_press(Message::Activate(index))
                        .interaction(iced::mouse::Interaction::Text),
                )
                .id(format!("block-{index}")),
            );
        }
        blocks = blocks
            .push(mouse_area(container(space()).height(160).width(Fill)).on_press(Message::Append));
        let page = container(blocks.max_width(width))
            .padding(32)
            .center_x(Fill);
        scrollable(page)
            .style(styles::scrollbar(tokens))
            .id(SCROLL)
            .height(Fill)
            .into()
    }

    fn editor_view(&self, source: bool) -> Element<'_, Message> {
        if !source {
            return RichEditor::new(
                self.editor.content(),
                &self.projection,
                self.document.source(),
                &self.resolved_theme,
                &self.images,
                editor_binding,
            )
            .collaboration(&self.document.collaboration)
            .preserve_viewport(
                self.preserve_viewport_generation == Some(self.document.collaboration.generation()),
            )
            .base(self.path.as_deref().and_then(std::path::Path::parent))
            .zoom(self.zoom)
            .selection(self.selection)
            .focused(self.editor_focused)
            .into();
        }
        let tokens = self.resolved_theme.tokens;
        let size = 14.5 * self.zoom;
        let base = text_editor(self.editor.content())
            .id(EDITOR)
            .on_action(Message::Edit)
            .font(Font::MONOSPACE)
            .size(size)
            .line_height(iced::widget::text::LineHeight::Relative(
                self.resolved_theme.typography.line_height,
            ))
            .padding([5, 0])
            .height(Fill)
            .min_height(36)
            .style(styles::editor(tokens))
            .placeholder("Begin anywhere…")
            .key_binding(editor_binding);
        let base = base.highlight(
            "md",
            if tokens.canvas.r < 0.5 {
                iced::highlighter::Theme::Base16Ocean
            } else {
                iced::highlighter::Theme::InspiredGitHub
            },
        );
        SourceEditor::new(base.into(), &self.document.collaboration, self.selection).into()
    }

    fn format_bar(&self) -> Element<'_, Message> {
        let mut toolbar = row![].spacing(3).align_y(Alignment::Center);
        for (kind, format) in [
            (Icon::Heading, Format::Heading),
            (Icon::Bold, Format::Bold),
            (Icon::Italic, Format::Italic),
            (Icon::Strikethrough, Format::Strikethrough),
            (Icon::Code, Format::Code),
            (Icon::Link, Format::Link),
            (Icon::Quote, Format::Quote),
            (Icon::List, Format::List),
            (Icon::Task, Format::Task),
            (Icon::Fence, Format::Fence),
            (Icon::Table, Format::Table),
        ] {
            let message = if matches!(format, Format::Quote) {
                Message::Writing(writing::Message::Details(writing::Kind::Quote))
            } else {
                Message::Format(format)
            };
            let hint = match format {
                Format::Heading => "Heading",
                Format::Bold => "Bold · ⌘ / Ctrl+B",
                Format::Italic => "Italic · ⌘ / Ctrl+I",
                Format::Strikethrough => "Strikethrough · ⌘ / Ctrl+Shift+X",
                Format::Code => "Inline code",
                Format::Link => "Link · ⌘ / Ctrl+K",
                Format::Quote => "Quotation and source credit",
                Format::List => "List",
                Format::Task => "Task",
                Format::Fence => "Code block",
                Format::Table => "Table",
            };
            toolbar = toolbar.push(self.icon_button(kind, hint, message, false));
        }
        toolbar = toolbar.push(self.icon_button(
            Icon::Image,
            "Add or edit image · paste or drop",
            Message::Writing(writing::Message::Details(writing::Kind::Image)),
            false,
        ));
        toolbar.height(32).into()
    }

    fn status_bar(&self) -> Element<'_, Message> {
        let tokens = self.resolved_theme.tokens;
        responsive(move |size| {
            let compact = size.width < 740.0;
            let mode = button(
                text(if self.mode == EditorMode::Source {
                    "Source"
                } else {
                    "Live"
                })
                .size(12),
            )
            .on_press(Message::ToggleSource)
            .padding([6, 10])
            .style(styles::button_style(
                tokens,
                if self.mode == EditorMode::Source {
                    Button::Selected
                } else {
                    Button::Quiet
                },
            ));
            let undo = button(icon::view(
                Icon::Undo,
                if self.document.can_undo() {
                    tokens.muted
                } else {
                    tokens.disabled
                },
            ))
            .on_press_maybe(self.document.can_undo().then_some(Message::Undo))
            .padding(5)
            .style(styles::button_style(tokens, Button::Quiet));
            let redo = button(icon::view(
                Icon::Redo,
                if self.document.can_redo() {
                    tokens.muted
                } else {
                    tokens.disabled
                },
            ))
            .on_press_maybe(self.document.can_redo().then_some(Message::Redo))
            .padding(5)
            .style(styles::button_style(tokens, Button::Quiet));
            let mut status = row![
                mode,
                self.small_button(
                    if self.mode == EditorMode::Read {
                        "Edit"
                    } else {
                        "Read"
                    },
                    Message::ToggleReading,
                    self.mode == EditorMode::Read
                ),
                undo,
                redo,
                space::horizontal(),
                text(format!("{} words", self.document.words))
                    .size(11)
                    .color(tokens.muted)
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            if !compact {
                status = status.push(
                    text(format!(
                        "·  {} min read",
                        self.document.words.div_ceil(200).max(1)
                    ))
                    .size(11)
                    .color(tokens.faint),
                );
            }
            status = status.push(space::horizontal()).push(
                text(if self.busy {
                    "Working…"
                } else if self.dirty {
                    "Unsaved"
                } else if self.path.is_some() {
                    "Saved"
                } else {
                    "Local file"
                })
                .size(11)
                .color(tokens.faint),
            );
            if !compact {
                status = status.push(self.small_button(
                    &self.resolved_theme.name,
                    Message::ShowSettings,
                    false,
                ));
            }
            container(status).padding([8, 22]).into()
        })
        .height(46)
        .into()
    }

    fn search_bar(&self) -> Element<'_, Message> {
        if !self.search_open {
            return space().height(0).into();
        }
        let tokens = self.resolved_theme.tokens;
        let count = if self.matches.is_empty() {
            "No matches".into()
        } else {
            format!("{} of {}", self.match_index + 1, self.matches.len())
        };
        let find = text_input("Find (case sensitive)", &self.query)
            .id(SEARCH)
            .on_input(Message::Query)
            .on_submit(Message::FindNext)
            .size(13)
            .padding(8)
            .style(styles::input(tokens));
        let replace = text_input("Replace with", &self.replacement)
            .on_input(Message::Replacement)
            .on_submit(Message::Replace)
            .size(13)
            .padding(8)
            .style(styles::input(tokens));
        container(
            column![
                row![
                    find,
                    text(count).size(11).color(tokens.muted),
                    self.small_button("Previous", Message::FindPrevious, false),
                    self.small_button("Next", Message::FindNext, false),
                    self.icon_button(Icon::Close, "Close find", Message::ToggleSearch, false)
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                row![
                    replace,
                    self.small_button("Replace", Message::Replace, false),
                    self.small_button("Replace all", Message::ReplaceAll, false)
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            ]
            .spacing(8),
        )
        .padding([12, 28])
        .style(styles::panel(tokens.surface))
        .into()
    }

    fn collaboration_view(&self) -> Element<'_, Message> {
        let tokens = self.resolved_theme.tokens;
        container(column![
            text("Offline collaboration").font(styles::BODY_FONT).size(28),
            text("Export a shared replica for offline writing, or a versioned source snapshot for an agent. Importing applies approved changes to this document.").size(13).color(tokens.muted),
            self.small_button("Open replica as offline copy…", Message::OpenReplica, false),
            self.small_button("Export replica…", Message::ExportReplica, false),
            self.small_button("Export agent snapshot…", Message::ExportAgentSnapshot, false),
            self.small_button("Merge approved replica…", Message::ImportReplica, false),
            self.small_button("Accept agent proposal…", Message::ImportProposal, false),
            row![self.small_button("Undo agent", Message::AgentUndo, false), self.small_button("Redo agent", Message::AgentRedo, false)].spacing(12),
            text(format!("Document {} · Writer {}", self.document.collaboration.id(), self.document.collaboration.replica_id())).size(10).color(tokens.faint),
            self.small_button("Done", Message::CloseOverlay, true),
        ].spacing(16)).padding(30).width(540).style(styles::card(tokens)).into()
    }

    fn settings_view(&self, height: f32) -> Element<'_, Message> {
        let tokens = self.resolved_theme.tokens;
        let heading = row![
            column![
                text("Make yourself at home.")
                    .font(styles::BODY_FONT)
                    .size(28),
                text("Choose the light you write in.")
                    .size(13)
                    .color(tokens.muted)
            ]
            .spacing(6),
            space::horizontal(),
            self.icon_button(
                Icon::Close,
                "Close appearance",
                Message::CloseOverlay,
                false
            )
        ]
        .align_y(Alignment::Center);
        let mut themes = column![].spacing(8);
        for (preset, preview) in Preset::ALL.into_iter().zip(&self.preset_themes) {
            let selected = self.preferences.custom_theme.is_none()
                && self.resolved_theme.name == preset.name();
            let swatch = container(
                column![
                    text("Aa")
                        .font(styles::BODY_FONT)
                        .size(25)
                        .color(preview.tokens.text),
                    container(space())
                        .height(3)
                        .width(28)
                        .style(styles::panel(preview.tokens.brand_accent))
                ]
                .spacing(4),
            )
            .padding([7, 14])
            .width(72)
            .style(styles::inset(preview.tokens));
            themes = themes.push(
                button(
                    row![
                        swatch,
                        column![
                            text(preset.name()).size(13),
                            text(preset.description()).size(12)
                        ]
                        .spacing(4),
                        space::horizontal(),
                        text(if selected { "Selected" } else { "" }).size(11)
                    ]
                    .spacing(14)
                    .align_y(Alignment::Center),
                )
                .width(Fill)
                .padding(8)
                .style(styles::button_style(
                    tokens,
                    if selected {
                        Button::Selected
                    } else {
                        Button::Quiet
                    },
                ))
                .on_press(Message::Theme(preset)),
            );
        }
        let mut shortcuts = column![].spacing(10);
        for (label, keys) in [
            ("New / Open / Save", "⌘/Ctrl N / O / S"),
            ("Save as", "⌘/Ctrl Shift S"),
            ("Find & replace", "⌘/Ctrl F"),
            ("Bold / Italic", "⌘/Ctrl B / I"),
            ("Strikethrough", "⌘/Ctrl Shift X"),
            ("Create / edit link", "⌘/Ctrl K"),
            ("Toggle current task", "⌘/Ctrl Enter"),
            ("Live preview / source", "⌘/Ctrl Shift M"),
            ("Focus mode", "⌘/Ctrl Shift F"),
            ("Previous / next block", "Alt ↑ / ↓"),
            ("Quick jump to heading", "⌘/Ctrl Shift J"),
            ("Move section up / down", "⌘/Ctrl Alt ↑ / ↓"),
            ("Collapse / expand section", "⌘/Ctrl Alt ← / →"),
            ("Close find or dialog", "Escape"),
            ("Undo / Redo", "⌘/Ctrl Z / Shift Z"),
            ("Text size", "⌘/Ctrl + / −"),
        ] {
            shortcuts = shortcuts.push(
                row![
                    text(label.to_owned()).size(12).color(tokens.muted),
                    space::horizontal(),
                    text(keys).size(11).color(tokens.faint)
                ]
                .spacing(20),
            );
        }
        let custom = if self.preferences.custom_theme.is_some() {
            format!("Custom theme · {}", self.resolved_theme.name)
        } else {
            "Load a custom theme (.toml)".to_owned()
        };
        let body = column![
            text("APPEARANCE").size(10).color(tokens.faint),
            themes,
            self.action_row(Icon::Folder, &custom, Message::LoadTheme),
            space().height(4),
            row![
                text("Text size").size(13),
                space::horizontal(),
                self.small_button("−", Message::Zoom(-0.1), false),
                text(format!("{}%", (self.zoom * 100.0).round() as u32))
                    .size(12)
                    .color(tokens.muted),
                self.small_button("+", Message::Zoom(0.1), false)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            space().height(8),
            text("KEYBOARD SHORTCUTS").size(10).color(tokens.faint),
            shortcuts
        ]
        .spacing(12)
        .padding(iced::Padding::new(2.0).right(16.0));
        container(
            column![
                heading,
                scrollable(body)
                    .height(Fill)
                    .style(styles::scrollbar(tokens)),
                row![
                    self.small_button("Collaboration…", Message::ShowCollaboration, false),
                    space::horizontal(),
                    self.small_button("Done", Message::CloseOverlay, true)
                ]
                .align_y(Alignment::Center)
            ]
            .spacing(22),
        )
        .padding(28)
        .width(560)
        .height(height)
        .style(styles::card(tokens))
        .into()
    }

    fn unsaved_view(&self) -> Element<'_, Message> {
        let tokens = self.resolved_theme.tokens;
        container(
            column![
                text("Keep this thought?").font(styles::BODY_FONT).size(28),
                text(format!(
                    "{} has unsaved changes. Save them before moving on.",
                    self.display_name
                ))
                .size(14)
                .color(tokens.muted),
                space().height(12),
                row![
                    button("Discard")
                        .on_press_maybe((!self.busy).then_some(Message::ConfirmDiscard))
                        .padding([10, 16])
                        .style(styles::button_style(tokens, Button::Danger)),
                    space::horizontal(),
                    self.small_button("Cancel", Message::CloseOverlay, false),
                    button(if self.busy {
                        "Saving…"
                    } else {
                        "Save changes"
                    })
                    .on_press_maybe((!self.busy).then_some(Message::ConfirmSave))
                    .padding([10, 16])
                    .style(styles::button_style(tokens, Button::Primary))
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            ]
            .spacing(16),
        )
        .padding(30)
        .width(500)
        .style(styles::card(tokens))
        .into()
    }

    fn icon_button<'a>(
        &self,
        kind: Icon,
        label: &'a str,
        message: Message,
        selected: bool,
    ) -> Element<'a, Message> {
        let tokens = self.resolved_theme.tokens;
        tooltip(
            button(icon::view(
                kind,
                if selected {
                    tokens.on_accent_soft
                } else {
                    tokens.muted
                },
            ))
            .padding(7)
            .style(styles::button_style(
                tokens,
                if selected {
                    Button::Selected
                } else {
                    Button::Quiet
                },
            ))
            .on_press(message),
            container(text(label).size(11))
                .padding([6, 9])
                .style(styles::card(tokens)),
            tooltip::Position::Bottom,
        )
        .into()
    }

    fn small_button<'a>(
        &self,
        label: &'a str,
        message: Message,
        selected: bool,
    ) -> Element<'a, Message> {
        button(text(label.to_owned()).size(12))
            .on_press(message)
            .padding([6, 10])
            .style(styles::button_style(
                self.resolved_theme.tokens,
                if selected {
                    Button::Selected
                } else {
                    Button::Quiet
                },
            ))
            .into()
    }

    fn action_row<'a>(&self, kind: Icon, label: &str, message: Message) -> Element<'a, Message> {
        button(
            row![
                icon::view(kind, self.resolved_theme.tokens.muted),
                text(label.to_owned()).size(12)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .on_press(message)
        .width(Fill)
        .padding([8, 4])
        .style(styles::button_style(
            self.resolved_theme.tokens,
            Button::Quiet,
        ))
        .into()
    }

    fn file_action<'a>(
        &self,
        label: &'a str,
        message: Message,
        selected: bool,
    ) -> Element<'a, Message> {
        let tokens = self.resolved_theme.tokens;
        button(
            row![
                icon::view(
                    if label == "Welcome" {
                        Icon::Book
                    } else {
                        Icon::Document
                    },
                    if selected {
                        tokens.on_accent_soft
                    } else {
                        tokens.faint
                    }
                ),
                text(label).size(13)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .on_press(message)
        .padding([11, 9])
        .width(Fill)
        .style(styles::button_style(
            tokens,
            if selected {
                Button::Selected
            } else {
                Button::Quiet
            },
        ))
        .into()
    }

    fn file_button(
        &self,
        name: &str,
        path: Option<PathBuf>,
        selected: bool,
    ) -> Element<'static, Message> {
        let tokens = self.resolved_theme.tokens;
        let mut file = button(
            row![
                icon::view(
                    Icon::Document,
                    if selected {
                        tokens.on_accent_soft
                    } else {
                        tokens.faint
                    }
                ),
                text(name.trim_end_matches(".md").to_owned()).size(13)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([11, 9])
        .width(Fill)
        .style(styles::button_style(
            tokens,
            if selected {
                Button::Selected
            } else {
                Button::Quiet
            },
        ));
        if let Some(path) = path {
            file = file.on_press(Message::OpenPath(path));
        }
        file.into()
    }
}

fn filename(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled.md".into())
}

struct BlockPrefix {
    length: usize,
    continuation: String,
}

fn block_prefix(line: &str) -> Option<BlockPrefix> {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let content = &line[indent..];
    let hashes = content.bytes().take_while(|&byte| byte == b'#').count();
    if (1..=6).contains(&hashes) && content.as_bytes().get(hashes) == Some(&b' ') {
        return Some(BlockPrefix {
            length: indent + hashes + 1,
            continuation: String::new(),
        });
    }
    if content.starts_with("> ") {
        return Some(BlockPrefix {
            length: indent + 2,
            continuation: format!("{}> ", &line[..indent]),
        });
    }
    if ["- ", "* ", "+ "]
        .iter()
        .any(|prefix| content.starts_with(prefix))
    {
        let task = content.get(2..).is_some_and(|tail| {
            tail.starts_with("[ ] ") || tail.starts_with("[x] ") || tail.starts_with("[X] ")
        });
        return Some(BlockPrefix {
            length: indent + if task { 6 } else { 2 },
            continuation: format!(
                "{}{}{}",
                &line[..indent],
                &content[..2],
                if task { "[ ] " } else { "" }
            ),
        });
    }
    let digits = content.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0
        && matches!(content.as_bytes().get(digits), Some(b'.' | b')'))
        && content.as_bytes().get(digits + 1) == Some(&b' ')
    {
        let number = content[..digits].parse::<u64>().ok()?.checked_add(1)?;
        return Some(BlockPrefix {
            length: indent + digits + 2,
            continuation: format!(
                "{}{}{} ",
                &line[..indent],
                number,
                &content[digits..digits + 1]
            ),
        });
    }
    None
}

fn editor_binding(press: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return None;
    }
    if press.modifiers.macos_command() && !press.modifiers.alt() {
        let motion = match press.key {
            Key::Named(key::Named::ArrowUp) => Some(text_editor::Motion::DocumentStart),
            Key::Named(key::Named::ArrowDown) => Some(text_editor::Motion::DocumentEnd),
            _ => None,
        };
        if let Some(motion) = motion {
            return Some(if press.modifiers.shift() {
                text_editor::Binding::Select(motion)
            } else {
                text_editor::Binding::Move(motion)
            });
        }
    }
    if matches!(press.key, Key::Named(key::Named::Tab)) {
        return Some(text_editor::Binding::Custom(Message::Edit(
            text_editor::Action::Edit(if press.modifiers.shift() {
                text_editor::Edit::Unindent
            } else {
                text_editor::Edit::Indent
            }),
        )));
    }
    if matches!(press.key, Key::Named(key::Named::Enter)) && press.modifiers.shift() {
        return Some(text_editor::Binding::Custom(Message::Edit(
            text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("\n".into()))),
        )));
    }
    if press.modifiers.command() && matches!(press.key.as_ref(), Key::Character("v")) {
        return Some(text_editor::Binding::Custom(Message::Writing(
            writing::Message::Paste(None),
        )));
    }
    if let Some(message) = shortcut(&press.key, press.modifiers) {
        return Some(text_editor::Binding::Custom(message));
    }
    text_editor::Binding::from_key_press(press)
}

fn event_message(event: Event, status: iced::event::Status, id: window::Id) -> Option<Message> {
    match event {
        Event::Window(window::Event::CloseRequested) => Some(Message::CloseRequested(id)),
        Event::Window(window::Event::FileDropped(path)) => Some(Message::Dropped(path)),
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(key::Named::Escape),
            ..
        }) if status == iced::event::Status::Captured => {
            Some(Message::Outline(outline::Message::CancelQuickJump))
        }
        Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
            if status == iced::event::Status::Ignored =>
        {
            shortcut(&key, modifiers).or_else(|| match key {
                Key::Named(key::Named::ArrowUp) if modifiers.is_empty() => {
                    Some(Message::Outline(outline::Message::Step(-1)))
                }
                Key::Named(key::Named::ArrowDown) if modifiers.is_empty() => {
                    Some(Message::Outline(outline::Message::Step(1)))
                }
                _ => None,
            })
        }
        Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
            Some(Message::Outline(outline::Message::CancelDrag))
        }
        Event::Window(window::Event::Unfocused) => {
            Some(Message::Outline(outline::Message::CancelDrag))
        }
        _ => None,
    }
}

fn shortcut(key: &Key, modifiers: keyboard::Modifiers) -> Option<Message> {
    match key.as_ref() {
        Key::Named(key::Named::Escape) => Some(Message::CloseOverlay),
        Key::Named(key::Named::Enter)
            if modifiers.command() && !modifiers.alt() && !modifiers.shift() =>
        {
            Some(Message::ToggleCurrentTask)
        }
        Key::Named(key::Named::ArrowUp) if modifiers.command() && modifiers.alt() => {
            Some(Message::Outline(outline::Message::Move(-1)))
        }
        Key::Named(key::Named::ArrowDown) if modifiers.command() && modifiers.alt() => {
            Some(Message::Outline(outline::Message::Move(1)))
        }
        Key::Named(key::Named::ArrowLeft) if modifiers.command() && modifiers.alt() => {
            Some(Message::Outline(outline::Message::FoldCurrent(true)))
        }
        Key::Named(key::Named::ArrowRight) if modifiers.command() && modifiers.alt() => {
            Some(Message::Outline(outline::Message::FoldCurrent(false)))
        }
        Key::Named(key::Named::ArrowUp) if modifiers.alt() => Some(Message::Navigate(-1)),
        Key::Named(key::Named::ArrowDown) if modifiers.alt() => Some(Message::Navigate(1)),
        Key::Character(character) if modifiers.command() => {
            match character.to_ascii_lowercase().as_str() {
                "n" => Some(Message::New),
                "o" => Some(Message::Open),
                "s" => Some(if modifiers.shift() {
                    Message::SaveAs
                } else {
                    Message::Save
                }),
                "f" => Some(if modifiers.shift() {
                    Message::ToggleFocus
                } else {
                    Message::ToggleSearch
                }),
                "b" => Some(Message::Format(Format::Bold)),
                "i" => Some(Message::Format(Format::Italic)),
                "x" if modifiers.shift() => Some(Message::Format(Format::Strikethrough)),
                "k" => Some(Message::Format(Format::Link)),
                "j" if modifiers.shift() => Some(Message::Outline(outline::Message::QuickJump)),
                "m" if modifiers.shift() => Some(Message::ToggleSource),
                "z" => Some(if modifiers.shift() {
                    Message::Redo
                } else {
                    Message::Undo
                }),
                "y" => Some(Message::Redo),
                "+" | "=" => Some(Message::Zoom(0.1)),
                "-" => Some(Message::Zoom(-0.1)),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{App, EditorMode, Format, Message, Overlay};
    use crate::document::selection::Selection;
    use crate::document::table_edit;
    use iced::widget::text_editor::{Action, Edit, Motion};
    use std::sync::Arc;

    fn app(source: &str) -> App {
        let (mut app, _task) = App::new();
        app.set_document(source.into(), None, "Test.md".into());
        app
    }
    #[test]
    fn code_edits_preserve_source_selection_and_atomic_history_in_both_modes() {
        for mode in [EditorMode::Live, EditorMode::Source] {
            let source = "Before\n\n```rust\nfn main() {}\n```\n\nAfter";
            let mut app = app(source);
            app.mode = mode;
            let at = source.find('}').unwrap();
            app.selection = Selection::caret(at);
            app.sync_editor();
            send(&mut app, Message::Edit(Action::Edit(Edit::Enter)));
            assert_eq!(
                app.document.source(),
                "Before\n\n```rust\nfn main() {\n    \n}\n```\n\nAfter"
            );
            assert_eq!(app.selection, Selection::caret(at + 5));
            send(&mut app, Message::Undo);
            assert_eq!(app.document.source(), source);
            assert_eq!(app.selection, Selection::caret(at));
            send(&mut app, Message::Redo);
            assert_eq!(app.selection, Selection::caret(at + 5));
            send(&mut app, Message::Edit(Action::Edit(Edit::Backspace)));
            assert_eq!(app.selection, Selection::caret(at + 1));
            send(&mut app, Message::Undo);
            assert_eq!(app.selection, Selection::caret(at + 5));
        }
    }
    #[test]
    fn empty_code_accepts_typing_paste_and_ime_without_consuming_its_fence() {
        for text in ["x", "日本語", "one\ntwo", "one\n"] {
            let source = "```rust\n```\n\nAfter";
            let mut app = app(source);
            let at = source.find('\n').unwrap() + 1;
            send(&mut app, Message::SelectSource(Selection::caret(at)));
            let target = app.document.collaboration.composition_target(app.selection);
            send(
                &mut app,
                Message::CommitComposition {
                    target,
                    text: text.into(),
                },
            );
            let separator = if text.ends_with('\n') { "" } else { "\n" };
            assert_eq!(
                app.document.source(),
                format!("```rust\n{text}{separator}```\n\nAfter")
            );
            assert_eq!(app.selection, Selection::caret(at + text.len()));
            assert_eq!(
                app.projection.rich.code_blocks[0].text.as_str(),
                format!("{text}{separator}")
            );
            send(&mut app, Message::Undo);
            assert_eq!(app.document.source(), source);
            assert_eq!(app.selection, Selection::caret(at));
        }
    }
    #[test]
    fn distant_disclosure_splices_preserve_native_text_reverse_selection_and_history() {
        let source = format!(
            "# First **title**\n\n{}Last *title*\n============\n",
            "Plain café 🦀 words.\n\n".repeat(600)
        );
        let mut app = app(&source);
        let first = source.find("First").unwrap() + 2;
        let last = source.find("Last").unwrap() + 2;
        for head in [first, last, first, last] {
            let selection = Selection {
                anchor: source.len(),
                head,
            };
            send(&mut app, Message::SelectSource(selection));
            assert_eq!(app.selection, selection);
            assert_eq!(app.editor.content().text(), app.projection.text);
            assert_eq!(app.document.source(), source);
            assert!(!app.dirty);
            let change = app.projection.change.as_ref().unwrap();
            assert!(change.regions.iter().map(|r| r.old.len()).sum::<usize>() < 100);
            assert_eq!(
                Selection::from_cursor(&app.projection.text, app.editor.content().cursor()),
                app.projection.display_selection(selection)
            );
        }
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), source);
    }

    fn send(app: &mut App, message: Message) {
        drop(app.update(message));
        assert_eq!(app.editor.snapshot(), app.editor.content().text());
        assert_eq!(
            app.editor.snapshot(),
            if app.mode == EditorMode::Source {
                app.document.source()
            } else {
                &app.projection.text
            }
        );
    }
    fn type_text(app: &mut App, text: &str) {
        for character in text.chars() {
            send(app, Message::Edit(Action::Edit(Edit::Insert(character))));
        }
    }
    fn select(app: &mut App, selection: Selection) {
        app.selection = app.projection.source_selection(selection, app.selection);
        app.editor.select(selection);
    }

    #[test]
    fn completed_markup_conceals_immediately_and_discloses_only_the_edited_span() {
        let mut app = app("");
        type_text(&mut app, "**bold** and *soft* and `code`");
        assert_eq!(app.document.source(), "**bold** and *soft* and `code`");
        assert_eq!(app.editor.content().text(), "bold and soft and code");
        send(&mut app, Message::SelectSource(Selection::caret(14)));
        assert_eq!(app.editor.content().text(), "bold and *soft* and code");
        send(&mut app, Message::Edit(Action::Move(Motion::DocumentEnd)));
        send(&mut app, Message::Edit(Action::Edit(Edit::Enter)));
        assert_eq!(app.editor.content().text(), "bold and soft and code\n");
        assert_eq!(app.editor.content().cursor().position.column, 0);
    }

    #[test]
    fn native_arrow_motion_can_reach_revealed_syntax() {
        let mut app = app("**bold**!\n\n**Next**");
        send(&mut app, Message::SelectSource(Selection::caret(2)));
        send(&mut app, Message::Edit(Action::Move(Motion::Left)));
        assert_eq!(app.selection, Selection::caret(1));
        send(&mut app, Message::Edit(Action::Move(Motion::Left)));
        assert_eq!(app.selection, Selection::caret(0));
        type_text(&mut app, "X");
        assert_eq!(app.document.source(), "X**bold**!\n\n**Next**");
        assert_eq!(app.editor.content().text(), "X**bold**!\n\nNext");
    }

    #[test]
    fn selection_and_replacement_cross_multiple_blocks_in_one_buffer() {
        let mut app = app("# First\n\n**Second**\n\nThird");
        select(
            &mut app,
            Selection {
                anchor: 3,
                head: 17,
            },
        );
        send(
            &mut app,
            Message::Edit(Action::Edit(Edit::Paste(Arc::new("X".into())))),
        );
        assert_eq!(app.editor.content().text(), "# FirXird");
        assert_eq!(app.document.source(), "# FirXird");
        send(&mut app, Message::Undo);
        assert_eq!(
            app.editor.content().selection().as_deref(),
            Some("st\n\nSecond\n\nTh")
        );
        send(&mut app, Message::Redo);
        assert_eq!(app.editor.content().text(), "# FirXird");
    }

    #[test]
    fn select_all_replacement_keeps_matching_prefix_and_clears_old_markup() {
        let mut app = app("# Same title\n\n**body**");
        send(&mut app, Message::Edit(Action::SelectAll));
        send(
            &mut app,
            Message::Edit(Action::Edit(Edit::Paste(Arc::new("Same title".into())))),
        );
        assert_eq!(app.document.source(), "Same title");
        assert_eq!(app.editor.content().text(), "Same title");
    }

    #[test]
    fn editing_a_visible_delimiter_and_undoing_restores_caret_and_source() {
        let mut app = app("**x**");
        send(&mut app, Message::SelectSource(Selection::caret(4)));
        send(&mut app, Message::Edit(Action::Edit(Edit::Delete)));
        assert_eq!(app.document.source(), "**x*");
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), "**x**");
        assert_eq!(app.selection, Selection::caret(4));
        send(&mut app, Message::Redo);
        assert_eq!(app.selection, Selection::caret(4));
        assert_eq!(
            app.editor.content().cursor().position.column,
            app.projection.to_display(4)
        );
    }

    #[test]
    fn source_toggle_preserves_reverse_unicode_selection() {
        let mut app = app("# 日本語\n\n**café 🦀**");
        let text = app.editor.content().text();
        select(
            &mut app,
            Selection {
                anchor: text.len(),
                head: 0,
            },
        );
        let original = app.editor.content().selection();
        send(&mut app, Message::ToggleSource);
        assert_eq!(app.mode, EditorMode::Source);
        assert!(app.editor.content().cursor().selection.is_some());
        send(&mut app, Message::ToggleSource);
        assert_eq!(app.mode, EditorMode::Live);
        assert_eq!(app.editor.content().selection(), original);
        assert_eq!(app.editor.content().cursor().position.column, 0);
    }

    #[test]
    fn unicode_commit_paste_is_one_lossless_edit() {
        let mut app = app("Before ");
        send(&mut app, Message::Edit(Action::Move(Motion::DocumentEnd)));
        send(
            &mut app,
            Message::Edit(Action::Edit(Edit::Paste(Arc::new(
                "日本語 e\u{301} 🧑‍💻".into(),
            )))),
        );
        assert_eq!(app.document.source(), "Before 日本語 e\u{301} 🧑‍💻");
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), "Before ");
    }

    #[test]
    fn incremental_unicode_edit_preserves_disclosure_cursor_and_reverse_history() {
        let source = "# Heading\n\nPlain **bold** and café.\n\n| a | b |\n|---|---|\n| x | y |\n\n![alt](missing.png)\n\nTail";
        let mut app = app(source);
        let point = source.find("café").unwrap() + 2;
        let before = Selection::caret(point);
        send(&mut app, Message::SelectSource(before));
        let old_tail = Arc::clone(app.projection.highlights.last().unwrap());
        let committed = "日本語 e\u{301}";
        send(
            &mut app,
            Message::Edit(Action::Edit(Edit::Paste(Arc::new(committed.into())))),
        );
        assert!(!app.document.block_changes.is_empty());
        assert!(app.projection.change.is_some());
        assert!(Arc::ptr_eq(
            &old_tail,
            app.projection.highlights.last().unwrap()
        ));
        assert_eq!(app.selection, Selection::caret(point + committed.len()));
        assert_eq!(app.editor.content().text(), app.projection.text);
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, before);
        assert!(!app.document.block_changes.is_empty());
        assert!(app.projection.change.is_some());

        let selection = Selection {
            anchor: point,
            head: source.find("bold").unwrap() + 2,
        };
        send(&mut app, Message::SelectSource(selection));
        send(
            &mut app,
            Message::Edit(Action::Edit(Edit::Paste(Arc::new(
                "new\n\nparagraph".into(),
            )))),
        );
        let after = app.document.source().to_owned();
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, selection);
        assert_eq!(
            Selection::from_cursor(&app.projection.text, app.editor.content().cursor()),
            app.projection.display_selection(selection)
        );
        send(&mut app, Message::Redo);
        assert_eq!(app.document.source(), after);
        assert_eq!(app.editor.content().text(), app.projection.text);
    }

    #[test]
    fn deferred_document_keeps_cross_block_selection_composition_and_history() {
        let source = format!(
            "Before café\n\n{}After 🦀 end.",
            "# Heading **title**\n\n| Name | Value |\n|---|---|\n| a | b |\n\n![alt](missing.png)\n\n".repeat(40)
        );
        let mut app = app(&source);
        let selection = Selection {
            anchor: source.rfind("After ").unwrap() + "After ".len(),
            head: "Before ".len(),
        };
        send(&mut app, Message::SelectSource(selection));
        for _ in 0..2 {
            send(&mut app, Message::ToggleSource);
            assert_eq!(app.selection, selection);
            send(&mut app, Message::ToggleSource);
            assert_eq!(app.selection, selection);
            assert_eq!(app.editor.content().text(), app.projection.text);
        }
        let committed = "日本語 e\u{301} 🧑‍💻\n";
        send(
            &mut app,
            Message::Edit(Action::Edit(Edit::Paste(Arc::new(committed.into())))),
        );
        assert_eq!(app.document.source(), format!("Before {committed}🦀 end."));
        let after = app.selection;
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, selection);
        assert_eq!(
            Selection::from_cursor(&app.projection.text, app.editor.content().cursor()),
            app.projection.display_selection(selection)
        );
        send(&mut app, Message::Redo);
        assert_eq!(app.document.source(), format!("Before {committed}🦀 end."));
        assert_eq!(app.selection, after);
    }

    #[test]
    fn source_only_search_matches_reveal_source_mode() {
        let mut app = app("A [label](https://example.com)");
        send(&mut app, Message::Query("example.com".into()));
        send(&mut app, Message::FindNext);
        assert_eq!(app.mode, EditorMode::Source);
        assert_eq!(
            app.editor.content().selection().as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn native_snapshot_preserves_crlf_reverse_replacement_and_history_across_modes() {
        let source = "# Heading **title**\r\n\r\nParagraph *café* 🦀.\r\n\r\nTail";
        let mut app = app(source);
        let selection = Selection {
            anchor: source.len(),
            head: source.find("Paragraph ").unwrap() + "Paragraph ".len(),
        };
        send(&mut app, Message::SelectSource(selection));
        send(&mut app, Message::ToggleSource);
        assert_eq!(app.editor.selection(), selection);
        let commit = "日本語 e\u{301} 🧑‍💻\r\nNext";
        send(
            &mut app,
            Message::Edit(Action::Edit(Edit::Paste(Arc::new(commit.into())))),
        );
        let after = format!("{}{commit}", &source[..selection.head]);
        let caret = Selection::caret(after.len());
        assert_eq!(app.document.source(), after);
        assert_eq!(app.selection, caret);
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, selection);
        assert_eq!(app.editor.selection(), selection);
        send(&mut app, Message::ToggleSource);
        assert_eq!(app.selection, selection);
        assert_eq!(
            app.editor.selection(),
            app.projection.display_selection(selection)
        );
        send(&mut app, Message::Redo);
        assert_eq!(app.document.source(), after);
        assert_eq!(app.selection, caret);
        send(&mut app, Message::Edit(Action::Move(Motion::DocumentStart)));
        send(&mut app, Message::Edit(Action::Select(Motion::DocumentEnd)));
        assert_eq!(
            app.selection,
            Selection {
                anchor: 0,
                head: after.len()
            }
        );
        send(&mut app, Message::Edit(Action::Move(Motion::Left)));
        assert!(app.selection.is_caret());
        assert!(app.editor.content().cursor().selection.is_none());
    }

    #[test]
    fn source_editing_and_file_safety_still_preserve_later_edits() {
        let mut app = app("Before");
        send(&mut app, Message::ToggleSource);
        send(&mut app, Message::Edit(Action::Move(Motion::DocumentEnd)));
        type_text(&mut app, "!");
        send(
            &mut app,
            Message::Saved {
                result: Ok("/tmp/test.md".into()),
                source: Arc::new("Before".into()),
                version: "test".into(),
                generation: 0,
            },
        );
        assert!(app.dirty);
        assert_eq!(app.document.source(), "Before!");
        send(&mut app, Message::New);
        assert!(matches!(app.overlay, Some(Overlay::Unsaved)));
        type_text(&mut app, "?");
        assert_eq!(app.document.source(), "Before!");
    }
    #[test]
    fn initial_caret_edits_heading_and_formatted_content_without_touching_prefixes() {
        let mut heading = app("# Title");
        type_text(&mut heading, "New ");
        assert_eq!(heading.document.source(), "# New Title");
        let mut bold = app("**word**");
        type_text(&mut bold, "New ");
        assert_eq!(bold.document.source(), "**New word**");
    }

    #[test]
    fn visible_heading_prefix_can_be_selected_deleted_and_undone() {
        let mut app = app("# Title");
        send(
            &mut app,
            Message::SelectSource(Selection { anchor: 0, head: 2 }),
        );
        send(&mut app, Message::Edit(Action::Edit(Edit::Delete)));
        assert_eq!(app.document.source(), "Title");
        assert_eq!(app.editor.content().text(), "Title");
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), "# Title");
    }

    #[test]
    fn all_heading_levels_reveal_and_support_literal_level_changes() {
        for level in 1..=6 {
            let prefix = "#".repeat(level);
            let source = format!("{prefix} Title\n\n## Other");
            let mut app = app(&source);
            send(&mut app, Message::EditorFocused(true));
            assert_eq!(
                app.editor.content().text(),
                format!("{prefix} Title\n\nOther")
            );
            send(&mut app, Message::Edit(Action::Move(Motion::Home)));
            assert_eq!(app.selection, Selection::caret(0));
            send(&mut app, Message::Edit(Action::Edit(Edit::Delete)));
            assert_eq!(app.document.source(), &source[1..]);
            if level > 1 {
                assert_eq!(app.projection.rich.headings[0].level, (level - 1) as u8);
            }
            send(&mut app, Message::Undo);
            assert_eq!(app.document.source(), source);
            assert_eq!(app.selection, Selection::caret(0));
        }
    }

    #[test]
    fn focus_disclosure_preserves_a_caret_inside_syntax_without_editing_history() {
        let mut app = app("### 日本語\n\n**Next**");
        send(&mut app, Message::SelectSource(Selection::caret(1)));
        let revision = app.document.revision;
        send(&mut app, Message::EditorFocused(false));
        assert_eq!(app.editor.content().text(), "日本語\n\nNext");
        assert_eq!(app.selection, Selection::caret(1));
        send(&mut app, Message::EditorFocused(true));
        assert_eq!(app.editor.content().text(), "### 日本語\n\nNext");
        assert_eq!(app.editor.content().cursor().position.column, 1);
        assert_eq!(app.document.revision, revision);
        assert!(!app.dirty);
        send(&mut app, Message::Undo);
        assert_eq!(app.document.revision, revision);
        assert_eq!(app.selection, Selection::caret(1));
    }

    #[test]
    fn geometric_caret_move_clears_native_selection_before_inserting_a_marker() {
        let mut app = app("# Title\n\nBody");
        send(
            &mut app,
            Message::SelectSource(Selection { anchor: 2, head: 7 }),
        );
        assert_eq!(app.editor.content().selection().as_deref(), Some("Title"));
        send(&mut app, Message::SelectSource(Selection::caret(0)));
        assert!(app.editor.content().cursor().selection.is_none());
        type_text(&mut app, "#");
        assert_eq!(app.document.source(), "## Title\n\nBody");
        send(
            &mut app,
            Message::SelectSource(Selection { anchor: 2, head: 8 }),
        );
        send(&mut app, Message::EditorFocused(false));
        send(&mut app, Message::SelectSource(Selection::caret(0)));
        assert!(app.editor.content().cursor().selection.is_none());
        type_text(&mut app, "#");
        assert_eq!(app.document.source(), "### Title\n\nBody");
    }

    #[test]
    fn disclosure_follows_selection_head_and_reuses_projection_within_a_context() {
        let source = "# 日本語\n\n**Next**";
        let mut app = app(source);
        send(&mut app, Message::SelectSource(Selection::caret(2)));
        let lines = Arc::clone(&app.projection.highlights);
        let snapshot = Arc::clone(&app.projection.source_id);
        send(&mut app, Message::Edit(Action::Move(Motion::Right)));
        assert!(Arc::ptr_eq(&lines, &app.projection.highlights));
        let selection = Selection {
            anchor: source.len(),
            head: 2,
        };
        send(&mut app, Message::SelectSource(selection));
        assert_eq!(app.editor.content().text(), "# 日本語\n\nNext");
        assert_eq!(app.selection, selection);
        let selection = Selection {
            anchor: 2,
            head: source.len() - 3,
        };
        send(&mut app, Message::SelectSource(selection));
        assert_eq!(app.editor.content().text(), "日本語\n\n**Next**");
        assert_eq!(app.selection, selection);
        assert!(!Arc::ptr_eq(&lines, &app.projection.highlights));
        assert!(Arc::ptr_eq(&snapshot, &app.projection.source_id));
    }

    #[test]
    fn setext_heading_reveals_its_editable_underline_with_the_title() {
        let mut app = app("Title\n=====\n\nAfter");
        send(&mut app, Message::EditorFocused(true));
        assert_eq!(app.editor.content().text(), "Title\n=====\n\nAfter");
        send(
            &mut app,
            Message::SelectSource(Selection {
                anchor: 6,
                head: 11,
            }),
        );
        type_text(&mut app, "-----");
        assert_eq!(app.projection.rich.headings[0].level, 2);
        send(&mut app, Message::SelectSource(Selection::caret(15)));
        assert_eq!(app.editor.content().text(), "Title\n\nAfter");
    }

    #[test]
    fn enter_continues_tasks_and_exits_an_empty_item() {
        let mut app = app("- [x] First");
        send(&mut app, Message::Edit(Action::Move(Motion::DocumentEnd)));
        send(&mut app, Message::Edit(Action::Edit(Edit::Enter)));
        assert_eq!(app.document.source(), "- [x] First\n- [ ] ");
        send(&mut app, Message::Edit(Action::Edit(Edit::Enter)));
        assert_eq!(app.document.source(), "- [x] First\n\n");
    }

    #[test]
    fn collapsing_select_all_does_not_leave_an_invisible_selection() {
        let mut app = app("**word**");
        send(&mut app, Message::Edit(Action::SelectAll));
        send(&mut app, Message::Edit(Action::Move(Motion::DocumentStart)));
        assert!(app.selection.is_caret());
        assert!(app.editor.content().selection().is_none());
    }

    #[test]
    #[ignore = "run with mise run bench"]
    fn performance_continuous_edit_projection() {
        use std::time::Instant;
        let source = (0..1000).map(|i| format!("## Heading {i}\n\nA paragraph with **bold** and [a link](https://example.com).\n\n")).collect::<String>();
        let start = Instant::now();
        let mut app = app(&source);
        let load = start.elapsed();
        send(&mut app, Message::Edit(Action::Move(Motion::DocumentEnd)));
        let start = Instant::now();
        for _ in 0..100 {
            drop(app.update(Message::Edit(Action::Edit(Edit::Insert('x')))));
        }
        let elapsed = start.elapsed();
        println!(
            "Continuous 2000 blocks: load={load:?}, 100 native-buffer/source/projection edits={elapsed:?}, mean={:?}",
            elapsed / 100
        );
        assert!(app.document.source().ends_with(&"x".repeat(100)));
        assert_eq!(app.editor.snapshot(), app.editor.content().text());
    }

    #[test]
    fn table_navigation_editing_and_rows_keep_source_and_history() {
        let source = "# Title\n\n| A | B |\n|---|---|\n| first | last |\n\nAfter";
        let mut app = app(source);
        let point = app.projection.rich.tables[0].rows[1][0].start;
        send(&mut app, Message::Select(Selection::caret(point)));
        let before = app.selection;
        send(&mut app, Message::Edit(Action::Edit(Edit::Insert('|'))));
        assert!(app.document.source().contains("\\|first"));
        assert_eq!(app.projection.rich.tables[0].alignment.len(), 2);
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, before);
        send(&mut app, Message::TableTab(false));
        type_text(&mut app, "New ");
        assert!(app.document.source().contains("| first | New last |"));
        send(&mut app, Message::Edit(Action::Edit(Edit::Enter)));
        assert_eq!(app.projection.rich.tables[0].rows.len(), 3);
        assert_eq!(
            table_edit::cell_at(&app.projection, app.selection)
                .map(|(_, row, column)| (row, column)),
            Some((2, 1))
        );
        type_text(&mut app, "Next");
        assert_eq!(
            &app.projection.text[app.projection.rich.tables[0].rows[2][1].clone()],
            "Next"
        );
        send(
            &mut app,
            Message::Edit(Action::Edit(Edit::Paste(Arc::new("\n".into())))),
        );
        type_text(&mut app, "Outside");
        assert_eq!(app.projection.rich.tables[0].rows.len(), 3);
        assert!(app.document.source().contains("\n\nOutside\nAfter"));
    }

    #[test]
    fn formatting_across_table_cells_and_image_preserves_structure() {
        let source =
            "# Title\n\n| A | B |\n|---|---|\n| left | right |\n\n![alt](room.png)\n\nAfter";
        let mut app = app(source);
        send(&mut app, Message::Edit(Action::SelectAll));
        send(&mut app, Message::Format(Format::Bold));
        assert!(app.document.source().contains("| **left** | **right** |"));
        assert!(app.document.source().contains("![alt](room.png)"));
        assert_eq!(app.projection.rich.tables.len(), 1);
        assert_eq!(app.projection.rich.images.len(), 1);
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), source);
    }

    #[test]
    fn strikethrough_formatting_preserves_reverse_selection_and_atomic_history() {
        let source = "Before café 🧑‍💻 after";
        let mut app = app(source);
        let before = Selection {
            anchor: source.find(" after").unwrap(),
            head: "Before ".len(),
        };
        send(&mut app, Message::SelectSource(before));
        send(&mut app, Message::Format(Format::Strikethrough));
        assert_eq!(app.document.source(), "Before ~~café 🧑‍💻~~ after");
        assert!(app.selection.anchor > app.selection.head);
        let marked = app.selection;
        send(&mut app, Message::ToggleSource);
        assert_eq!(app.selection, marked);
        send(&mut app, Message::ToggleSource);
        assert_eq!(app.selection, marked);
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, before);
        send(&mut app, Message::Redo);
        assert_eq!(app.document.source(), "Before ~~café 🧑‍💻~~ after");
        assert_eq!(app.selection, marked);
        send(&mut app, Message::Format(Format::Strikethrough));
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, before);
    }

    #[test]
    fn strikethrough_shortcut_keeps_ordinary_cut_available() {
        use super::shortcut;
        use iced::keyboard::{self, Key};

        let command = if cfg!(target_os = "macos") {
            keyboard::Modifiers::LOGO
        } else {
            keyboard::Modifiers::CTRL
        };
        let key = Key::Character("x".into());
        assert!(matches!(
            shortcut(&key, command | keyboard::Modifiers::SHIFT),
            Some(Message::Format(Format::Strikethrough))
        ));
        assert!(shortcut(&key, command).is_none());
    }

    #[test]
    fn image_arrow_selection_delete_and_undo_are_atomic() {
        let source = "# Title\n\n![alt](room.png)\n\nAfter";
        let mut app = app(source);
        let image = app.projection.rich.images[0].range.clone();
        send(&mut app, Message::Select(Selection::caret(image.end)));
        send(&mut app, Message::Edit(Action::Select(Motion::Left)));
        assert_eq!(
            app.editor.content().selection().as_deref(),
            Some("\u{fffc}")
        );
        let selected = app.selection;
        send(&mut app, Message::ToggleSource);
        assert_eq!(
            app.editor.content().selection().as_deref(),
            Some("![alt](room.png)")
        );
        send(&mut app, Message::ToggleSource);
        assert_eq!(app.selection, selected);
        send(&mut app, Message::Edit(Action::Edit(Edit::Delete)));
        assert!(app.projection.rich.images.is_empty());
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, selected);
    }
}
