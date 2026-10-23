//! Debounced recovery scheduling. Late writes from a retired document are removed
//! after completion, so saving/discarding cannot resurrect an obsolete draft.

use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Task};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use uuid::Uuid;

use super::{App, Message as AppMessage, Overlay, Pending};
use crate::document::{Document, selection::Selection};
use crate::io::{recovery, storage};
use crate::theme::styles;

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    Available(Result<bool, storage::FileError>),
    OpenFolder,
    Show,
    Listed(Result<Vec<recovery::Entry>, storage::FileError>),
    Restore(Uuid),
    Loaded(Result<recovery::Snapshot, storage::FileError>),
    Written {
        id: Uuid,
        generation: u64,
        result: Result<(), storage::FileError>,
    },
    Removed(Result<(), storage::FileError>),
}
pub(super) struct State {
    id: Uuid,
    edited: Instant,
    last_checkpoint: Instant,
    saved: Option<u64>,
    inflight: Option<Uuid>,
    retired: HashSet<Uuid>,
    cleanup: Vec<Uuid>,
    entries: Vec<recovery::Entry>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            edited: Instant::now(),
            last_checkpoint: Instant::now(),
            saved: None,
            inflight: None,
            retired: HashSet::new(),
            cleanup: Vec::new(),
            entries: Vec::new(),
        }
    }
}
impl State {
    pub fn needs_checkpoint(&self, generation: u64) -> bool {
        self.saved != Some(generation) && self.inflight.is_none()
    }
    fn checkpoint_due(&self, generation: u64, now: Instant) -> bool {
        self.needs_checkpoint(generation)
            && (now.saturating_duration_since(self.edited) >= Duration::from_secs(2)
                || now.saturating_duration_since(self.last_checkpoint) >= Duration::from_secs(30))
    }
    pub fn edited(&mut self) {
        self.edited = Instant::now();
    }
    pub fn retire(&mut self) {
        let id = std::mem::replace(&mut self.id, Uuid::new_v4());
        self.saved = None;
        self.edited = Instant::now();
        self.last_checkpoint = self.edited;
        if self.inflight == Some(id) {
            self.retired.insert(id);
        } else {
            self.cleanup.push(id);
        }
    }
    fn written(&mut self, id: Uuid, generation: u64, success: bool) {
        if self.inflight == Some(id) {
            self.inflight = None;
        }
        if self.retired.remove(&id) {
            self.cleanup.push(id);
        } else if id == self.id && success {
            self.saved = Some(generation);
        }
    }
}
fn root() -> Option<std::path::PathBuf> {
    storage::preferences_path().and_then(|path| path.parent().map(|p| p.join("recovery")))
}
impl App {
    pub(super) fn recovery_discovery(&self) -> Task<AppMessage> {
        let Some(root) = root() else {
            return Task::none();
        };
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    recovery::list(&root).map(|entries| !entries.is_empty())
                })
                .await
                .map_err(|e| storage::FileError::Io(e.to_string()))?
            },
            |result| AppMessage::Recovery(Message::Available(result)),
        )
    }
    pub(super) fn recovery_cleanup(&mut self) -> Task<AppMessage> {
        let ids = std::mem::take(&mut self.recovery.cleanup);
        if ids.is_empty() {
            return Task::none();
        }
        let Some(root) = root() else {
            return Task::none();
        };
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    for id in ids {
                        recovery::remove(&root, id)?;
                    }
                    Ok(())
                })
                .await
                .map_err(|e| storage::FileError::Io(e.to_string()))?
            },
            |result| AppMessage::Recovery(Message::Removed(result)),
        )
    }
    pub(super) fn update_recovery(&mut self, message: Message) -> Task<AppMessage> {
        let Some(root) = root() else {
            return Task::none();
        };
        match message {
            Message::Available(result) => match result {
                Ok(true) if self.notification.is_none() => {
                    self.notification = Some("Recovery copies are available in the sidebar.".into())
                }
                Err(error) => tracing::warn!(%error, "Could not list recovery copies"),
                _ => {}
            },
            Message::OpenFolder => {
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
                            open::that_detached(root).map_err(|e| e.to_string())
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    AppMessage::LinkOpened,
                );
            }
            Message::Tick => {
                let generation = self.document.collaboration.generation();
                if self.dirty
                    && !self.loading
                    && self.recovery.checkpoint_due(generation, Instant::now())
                {
                    let snapshot = recovery::Snapshot {
                        state: self.document.collaboration.state(),
                        path: self.path.clone(),
                        name: self.display_name.clone(),
                        baseline: self.saved_source.clone(),
                        baseline_version: self.saved_version.clone(),
                        anchor: self.selection.anchor,
                        head: self.selection.head,
                    };
                    let id = self.recovery.id;
                    self.recovery.inflight = Some(id);
                    self.recovery.last_checkpoint = Instant::now();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                recovery::write(&root, id, &snapshot)
                            })
                            .await
                            .map_err(|e| storage::FileError::Io(e.to_string()))?
                        },
                        move |result| {
                            AppMessage::Recovery(Message::Written {
                                id,
                                generation,
                                result,
                            })
                        },
                    );
                }
            }
            Message::Written {
                id,
                generation,
                result,
            } => {
                self.recovery.written(id, generation, result.is_ok());
                if let Err(error) = result {
                    self.notification = Some(format!("Recovery copy could not be saved: {error}"));
                }
            }
            Message::Show => {
                if self.overlay.is_some() || self.loading {
                    return Task::none();
                }
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || recovery::list(&root))
                            .await
                            .map_err(|e| storage::FileError::Io(e.to_string()))?
                    },
                    |result| AppMessage::Recovery(Message::Listed(result)),
                );
            }
            Message::Listed(result) => match result {
                Ok(entries) if self.overlay.is_none() => {
                    self.recovery.entries = entries;
                    self.overlay = Some(Overlay::Recovery);
                }
                Err(error) => self.notification = Some(error.to_string()),
                _ => {}
            },
            Message::Restore(id) => {
                if !matches!(self.overlay, Some(Overlay::Recovery)) || self.busy {
                    return Task::none();
                }
                self.busy = true;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || recovery::read(&root, id))
                            .await
                            .map_err(|e| storage::FileError::Io(e.to_string()))?
                    },
                    |result| AppMessage::Recovery(Message::Loaded(result)),
                );
            }
            Message::Loaded(result) => {
                self.busy = false;
                if !matches!(self.overlay, Some(Overlay::Recovery)) {
                    return Task::none();
                }
                match result {
                    Ok(snapshot) => {
                        self.overlay = None;
                        return self.request(Pending::Recover(Box::new(snapshot)));
                    }
                    Err(error) => self.notification = Some(error.to_string()),
                }
            }
            Message::Removed(result) => {
                if let Err(error) = result {
                    tracing::warn!(%error, "Could not remove retired recovery copy");
                }
            }
        }
        Task::none()
    }
    pub(super) fn restore_draft(&mut self, snapshot: recovery::Snapshot) {
        match Document::from_state(&snapshot.state) {
            Ok(document) => {
                self.install_document(document, snapshot.path, snapshot.name);
                self.saved_source = snapshot.baseline;
                self.saved_version = snapshot.baseline_version;
                self.selection = Selection {
                    anchor: snapshot.anchor,
                    head: snapshot.head,
                }
                .clamp(self.document.source());
                self.dirty = true;
                self.sync_editor();
                self.notification = Some("Recovery copy opened. Save checks the last disk version; Save As keeps both. The original recovery copy is retained.".into());
            }
            Err(error) => self.notification = Some(error.to_string()),
        }
    }
    pub(super) fn recovery_view(&self) -> Element<'_, AppMessage> {
        let tokens = self.resolved_theme.tokens;
        let mut entries = column![].spacing(10);
        for entry in &self.recovery.entries {
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_sub(entry.modified);
            let age = if seconds < 60 {
                "Just now".into()
            } else if seconds < 3600 {
                format!("{} min ago", seconds / 60)
            } else if seconds < 86400 {
                format!("{} h ago", seconds / 3600)
            } else {
                format!("{} d ago", seconds / 86400)
            };
            entries = entries.push(
                row![
                    column![
                        text(&entry.name).size(15),
                        text(age).size(11).color(tokens.muted)
                    ]
                    .spacing(4)
                    .width(iced::Fill),
                    self.small_button(
                        "Open copy",
                        AppMessage::Recovery(Message::Restore(entry.id)),
                        false
                    )
                ]
                .spacing(12),
            );
        }
        if self.recovery.entries.is_empty() {
            entries = entries.push(
                text("No recovery copies are available.")
                    .size(14)
                    .color(tokens.muted),
            );
        }
        let content = column![
            text("Recovery copies").font(styles::BODY_FONT).size(28),
            text("Unsaved writing is checkpointed after two idle seconds, or every 30 seconds while you keep writing. Open a copy to inspect it; your Markdown is never overwritten automatically. Earlier copies are retained in the recovery folder.").size(13).color(tokens.muted),
            scrollable(entries).height(iced::Fill),
            row![self.small_button("Show folder", AppMessage::Recovery(Message::OpenFolder), false), self.small_button("Close", AppMessage::CloseOverlay, false)].spacing(8)
        ].spacing(20).height(iced::Fill);
        container(content)
            .width(560)
            .height(540)
            .padding(28)
            .style(styles::panel(tokens.surface))
            .into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_write_cannot_resurrect_saved_or_discarded_document() {
        let mut state = State::default();
        let old = state.id;
        state.inflight = Some(old);
        state.retire();
        assert!(state.cleanup.is_empty());
        assert_ne!(old, state.id);
        state.written(old, 4, true);
        assert_eq!(state.cleanup, vec![old]);
        assert_eq!(state.saved, None);
        state.written(state.id, 9, true);
        assert_eq!(state.saved, Some(9));
        assert!(!state.needs_checkpoint(9));
        assert!(state.needs_checkpoint(10));
        let start = Instant::now();
        state.last_checkpoint = start;
        state.edited = start;
        assert!(!state.checkpoint_due(10, start + Duration::from_secs(1)));
        assert!(state.checkpoint_due(10, start + Duration::from_secs(2)));
        state.edited = start + Duration::from_secs(29);
        assert!(
            state.checkpoint_due(10, start + Duration::from_secs(30)),
            "continuous typing must still be checkpointed"
        );
    }
    #[tokio::test]
    async fn restored_draft_keeps_its_disk_conflict_guard_and_fresh_replica_identity() {
        use crate::io::collaboration_store;
        use iced::widget::text_editor::{Action, Edit};
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        let (mut app, _) = App::new();
        app.set_document(
            "# Saved\r\n\r\nOriginal café.\r\n".into(),
            Some(path.clone()),
            "note.md".into(),
        );
        let state = app.document.collaboration.state();
        app.saved_version = Some(collaboration_store::version(&state));
        collaboration_store::save_document(
            path.clone(),
            app.document.source().into(),
            state,
            None,
            None,
        )
        .await
        .unwrap();
        let selection = Selection {
            anchor: app.document.source().len(),
            head: 2,
        };
        app.selection = selection;
        app.sync_editor();
        drop(
            app.update(AppMessage::Edit(Action::Edit(Edit::Paste(Arc::new(
                "Draft 海 👩‍💻".into(),
            ))))),
        );
        let snapshot = recovery::Snapshot {
            state: app.document.collaboration.state(),
            path: app.path.clone(),
            name: app.display_name.clone(),
            baseline: app.saved_source.clone(),
            baseline_version: app.saved_version.clone(),
            anchor: 10,
            head: 2,
        };
        let expected = app.document.source().to_owned();
        let writer = app.document.collaboration.replica_id();
        let id = Uuid::new_v4();
        recovery::write(dir.path(), id, &snapshot).unwrap();
        std::fs::write(&path, "External work must survive.").unwrap();
        app.restore_draft(recovery::read(dir.path(), id).unwrap());
        assert_eq!(app.document.source(), expected);
        assert_eq!(
            app.document.source(),
            app.document.collaboration.materialize()
        );
        assert_eq!(app.editor.snapshot(), app.projection.text);
        assert!(app.selection.anchor > app.selection.head);
        assert_ne!(app.document.collaboration.replica_id(), writer);
        assert!(app.dirty);
        let result = collaboration_store::save_document(
            path.clone(),
            app.document.source().into(),
            app.document.collaboration.state(),
            Some(app.saved_source.clone()),
            app.saved_version.clone(),
        )
        .await;
        assert!(matches!(result, Err(storage::FileError::Conflict)));
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "External work must survive."
        );
    }
}
