//! Outline disclosure, heading search, and section commands.

use iced::widget::{button, column, container, operation, row, space, text, text_input, tooltip};
use iced::{Alignment, Element, Fill, Task};

use super::{App, EDITOR, EditorMode, Message as AppMessage, Overlay};
use crate::document::collaboration::{EditOrigin, StableSelection};
use crate::document::outline::{self, DropTarget, Section};
use crate::document::selection::Selection;
use crate::editor::outline_row::OutlineRow;
use crate::theme::styles::{self, Button};

const QUICK_JUMP: &str = "outline-quick-jump";

#[cfg(test)]
#[path = "outline_tests.rs"]
mod tests;

#[derive(Debug, Clone)]
pub enum Message {
    Toggle(usize),
    FoldAll(bool),
    FoldCurrent(bool),
    Jump(usize),
    Move(i32),
    BeginDrag(usize),
    Hover(DropTarget),
    Leave(usize),
    Drop(DropTarget),
    CancelDrag,
    QuickJump,
    Query(String),
    Step(i32),
    Accept,
    CancelQuickJump,
}

#[derive(Default)]
pub(super) struct State {
    revision: Option<u64>,
    pub sections: Vec<Section>,
    // Moving text creates new CRDT identities. Retain both anchors so folds
    // follow the section through move, undo, and redo. Deleted anchors collapse
    // and cannot accidentally fold the next heading.
    collapsed: Vec<Vec<StableSelection>>,
    folded: Vec<bool>,
    pub drag: Option<(usize, u64)>,
    pub hover: Option<DropTarget>,
    query: String,
    results: Vec<usize>,
    selected: usize,
}

impl State {
    fn visible(&self) -> Vec<usize> {
        let mut result = Vec::new();
        let mut index = 0;
        while index < self.sections.len() {
            result.push(index);
            index += if self.folded[index] {
                self.sections[index].descendants + 1
            } else {
                1
            };
        }
        result
    }

    fn search(&mut self) {
        let query = self.query.to_lowercase();
        let terms: Vec<_> = query.split_whitespace().collect();
        if terms.is_empty() {
            self.results = (0..self.sections.len()).collect();
            self.selected = self.selected.min(self.results.len().saturating_sub(1));
            return;
        }
        let mut matches: Vec<_> = self
            .sections
            .iter()
            .enumerate()
            .filter_map(|(index, section)| {
                let title = section.title.to_lowercase();
                let mut label = title.clone();
                let mut parent = section.parent;
                while let Some(index) = parent {
                    label.push(' ');
                    label.push_str(&self.sections[index].title.to_lowercase());
                    parent = self.sections[index].parent;
                }
                terms
                    .iter()
                    .all(|term| label.contains(term))
                    .then_some((!terms.iter().all(|term| title.contains(term)), index))
            })
            .collect();
        matches.sort_unstable();
        self.results = matches.into_iter().map(|(_, index)| index).collect();
        self.selected = self.selected.min(self.results.len().saturating_sub(1));
    }

    fn cancel_drag(&mut self) {
        self.drag = None;
        self.hover = None;
    }
}

impl App {
    pub(super) fn sync_outline(&mut self) {
        if self.outline.revision == Some(self.document.revision) {
            return;
        }
        self.outline.cancel_drag();
        self.outline.sections = outline::sections(&self.document);
        self.outline.revision = Some(self.document.revision);
        self.refresh_folds();
        if matches!(self.overlay, Some(Overlay::QuickJump)) {
            self.outline.search();
        } else {
            self.outline.results.clear();
        }
    }

    fn folded_section(&self, anchors: &[StableSelection]) -> Option<usize> {
        anchors.iter().find_map(|anchor| {
            let selection = self.document.collaboration.resolve_selection(anchor)?;
            if selection.is_caret() {
                return None;
            }
            self.outline
                .sections
                .binary_search_by_key(&selection.range().start, |s| s.range.start)
                .ok()
        })
    }

    fn refresh_folds(&mut self) {
        self.outline.folded = vec![false; self.outline.sections.len()];
        for anchors in &self.outline.collapsed {
            if let Some(index) = self.folded_section(anchors) {
                self.outline.folded[index] = true;
            }
        }
    }

    fn fold_section(&mut self, index: usize, folded: bool) {
        let Some(section) = self.outline.sections.get(index) else {
            return;
        };
        if section.descendants == 0 {
            return;
        }
        if folded && !self.outline.folded[index] {
            let start = section.range.start;
            let end = start
                + self.document.source()[start..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8);
            self.outline
                .collapsed
                .push(vec![self.document.collaboration.selection(Selection {
                    anchor: start,
                    head: end,
                })]);
        } else if !folded {
            let remove: Vec<_> = self
                .outline
                .collapsed
                .iter()
                .enumerate()
                .filter_map(|(slot, anchors)| {
                    (self.folded_section(anchors) == Some(index)).then_some(slot)
                })
                .collect();
            for slot in remove.into_iter().rev() {
                self.outline.collapsed.remove(slot);
            }
        }
        self.outline.folded[index] = folded;
    }

    pub(super) fn update_outline(&mut self, message: Message) -> Task<AppMessage> {
        if matches!(message, Message::CancelQuickJump) {
            self.outline.cancel_drag();
        }
        if matches!(message, Message::CancelDrag) {
            self.outline.cancel_drag();
            return Task::none();
        }
        if self.loading {
            return Task::none();
        }
        let quick = matches!(self.overlay, Some(Overlay::QuickJump));
        if self.overlay.is_some()
            && !(quick
                && matches!(
                    message,
                    Message::Query(_)
                        | Message::Step(_)
                        | Message::Accept
                        | Message::Jump(_)
                        | Message::CancelQuickJump
                ))
        {
            return Task::none();
        }
        match message {
            Message::Toggle(index) => {
                if let Some(folded) = self.outline.folded.get(index).copied() {
                    self.fold_section(index, !folded);
                }
            }
            Message::FoldAll(folded) => {
                if folded {
                    for index in 0..self.outline.sections.len() {
                        self.fold_section(index, true);
                    }
                } else {
                    self.outline.collapsed.clear();
                    self.refresh_folds();
                }
            }
            Message::FoldCurrent(folded) => {
                if let Some(mut index) =
                    outline::current(&self.outline.sections, self.selection.head)
                {
                    if !folded {
                        let mut parent = self.outline.sections[index].parent;
                        while let Some(ancestor) = parent {
                            if self.outline.folded[ancestor] {
                                index = ancestor;
                            }
                            parent = self.outline.sections[ancestor].parent;
                        }
                    }
                    if folded
                        && (self.outline.sections[index].descendants == 0
                            || self.outline.folded[index])
                    {
                        index = self.outline.sections[index].parent.unwrap_or(index);
                    }
                    self.fold_section(index, folded);
                }
            }
            Message::Jump(index) => {
                let Some(section) = self.outline.sections.get(index) else {
                    return Task::none();
                };
                let block = section.block;
                let mut parent = section.parent;
                while let Some(index) = parent {
                    parent = self.outline.sections[index].parent;
                    self.fold_section(index, false);
                }
                self.overlay = None;
                self.outline.cancel_drag();
                if self.mode == EditorMode::Read {
                    self.selection = Selection::caret(self.document.blocks[block].range.start);
                    self.document.break_history_group();
                    self.sync_editor();
                }
                return self.update_message(AppMessage::JumpTo(block));
            }
            Message::Move(direction) => {
                if let Some(from) = outline::current(&self.outline.sections, self.selection.head)
                    && let Some(section) = outline::sibling(&self.outline.sections, from, direction)
                {
                    return self.move_section(
                        from,
                        DropTarget {
                            section,
                            after: direction > 0,
                        },
                    );
                }
            }
            Message::BeginDrag(index) => {
                if index < self.outline.sections.len() {
                    self.outline.drag = Some((index, self.document.revision));
                    self.outline.hover = None;
                }
            }
            Message::Hover(target) => {
                self.outline.hover = self.outline.drag.and_then(|(from, revision)| {
                    (revision == self.document.revision
                        && outline::destination(&self.outline.sections, from, target).is_some())
                    .then_some(target)
                });
            }
            Message::Leave(index) => {
                if self
                    .outline
                    .hover
                    .is_some_and(|target| target.section == index)
                {
                    self.outline.hover = None;
                }
            }
            Message::Drop(target) => {
                let drag = self.outline.drag;
                self.outline.cancel_drag();
                if let Some((from, revision)) = drag
                    && revision == self.document.revision
                {
                    return self.move_section(from, target);
                }
            }
            Message::CancelDrag => {}
            Message::QuickJump => {
                self.outline.cancel_drag();
                self.document.break_history_group();
                self.outline.query.clear();
                self.outline.selected = 0;
                self.outline.search();
                self.overlay = Some(Overlay::QuickJump);
                return operation::focus(QUICK_JUMP);
            }
            Message::Query(query) if quick => {
                self.outline.query = query;
                self.outline.selected = 0;
                self.outline.search();
            }
            Message::Step(direction) if quick => {
                self.outline.selected = self
                    .outline
                    .selected
                    .saturating_add_signed(direction as isize)
                    .min(self.outline.results.len().saturating_sub(1));
            }
            Message::Accept if quick => {
                if let Some(&index) = self.outline.results.get(self.outline.selected) {
                    return self.update_outline(Message::Jump(index));
                }
            }
            Message::CancelQuickJump if quick => {
                return self.update_message(AppMessage::CloseOverlay);
            }
            Message::CancelQuickJump => {}
            Message::Query(_) | Message::Step(_) | Message::Accept => {}
        }
        Task::none()
    }

    fn move_section(&mut self, from: usize, target: DropTarget) -> Task<AppMessage> {
        let movement =
            match outline::prepare(self.document.source(), &self.outline.sections, from, target) {
                Ok(Some(movement)) => movement,
                Ok(None) => return Task::none(),
                Err(error) => {
                    self.notification = Some(error.into());
                    return Task::none();
                }
            };
        let source_range = self.outline.sections[from].range.clone();
        let folds: Vec<_> = self
            .outline
            .collapsed
            .iter()
            .enumerate()
            .filter_map(|(slot, anchors)| {
                let index = self.folded_section(anchors)?;
                let start = self.outline.sections[index].range.start;
                source_range
                    .contains(&start)
                    .then_some((slot, movement.offset(start, false)))
            })
            .collect();
        let after = movement.selection(self.selection);
        match self.document.transact(
            &movement.edits,
            EditOrigin::Human,
            Some((self.selection, after)),
            true,
        ) {
            Ok(()) => {
                self.document.break_history_group();
                for (slot, start) in folds {
                    let end = start
                        + self.document.source()[start..]
                            .chars()
                            .next()
                            .map_or(0, char::len_utf8);
                    self.outline.collapsed[slot].push(self.document.collaboration.selection(
                        Selection {
                            anchor: start,
                            head: end,
                        },
                    ));
                }
                self.selection = after;
                if self.mode == EditorMode::Read {
                    self.mode = EditorMode::Live;
                }
                self.editor_focused = true;
                self.sync_editor();
                self.changed();
                operation::focus(EDITOR)
            }
            Err(error) => {
                self.notification = Some(error.to_string());
                Task::none()
            }
        }
    }

    pub(super) fn outline_list(&self) -> Element<'_, AppMessage> {
        let tokens = self.resolved_theme.tokens;
        let mut list = column![
            button(text("Quick jump…").size(13))
                .width(Fill)
                .on_press(AppMessage::Outline(Message::QuickJump))
                .style(styles::button_style(tokens, Button::Quiet)),
            row![
                button(text("Collapse all").size(11))
                    .on_press(AppMessage::Outline(Message::FoldAll(true)))
                    .style(styles::button_style(tokens, Button::Quiet)),
                button(text("Expand all").size(11))
                    .on_press(AppMessage::Outline(Message::FoldAll(false)))
                    .style(styles::button_style(tokens, Button::Quiet))
            ]
            .spacing(4),
        ]
        .spacing(4);
        let current = outline::current(&self.outline.sections, self.selection.head);
        let visible = self.outline.visible();
        let indicator = self.outline.hover.map(|target| {
            if target.after {
                let end = target.section + self.outline.sections[target.section].descendants;
                DropTarget {
                    section: visible[visible.partition_point(|&index| index <= end) - 1],
                    after: true,
                }
            } else {
                target
            }
        });
        for index in visible {
            let section = &self.outline.sections[index];
            let active = current.is_some_and(|current| {
                current == index
                    || self.outline.folded[index]
                        && current > index
                        && current <= index + section.descendants
            });
            let disclosure: Element<'_, AppMessage> = if section.descendants > 0 {
                button(
                    text(if self.outline.folded[index] {
                        "▸"
                    } else {
                        "▾"
                    })
                    .size(12),
                )
                .padding([5, 4])
                .width(22)
                .on_press(AppMessage::Outline(Message::Toggle(index)))
                .style(styles::button_style(tokens, Button::Quiet))
                .into()
            } else {
                space().width(22).into()
            };
            let title = if section.title.is_empty() {
                "Untitled heading"
            } else {
                &section.title
            };
            let label = button(text(title).size(13))
                .padding([6, 3])
                .width(Fill)
                .on_press(AppMessage::Outline(Message::Jump(index)))
                .style(styles::button_style(
                    tokens,
                    if active {
                        Button::Selected
                    } else {
                        Button::Quiet
                    },
                ));
            let handle = tooltip(
                container(text("⠿").size(15).color(tokens.faint))
                    .width(22)
                    .center_y(Fill),
                text("Drag before/after a heading of the same level").size(12),
                tooltip::Position::Right,
            );
            let contents = container(row![disclosure, label, handle].align_y(Alignment::Center))
                .id(format!("outline-section-{index}"))
                .padding(iced::Padding::ZERO.left((section.level - 1) as f32 * 10.0));
            list = list.push(
                OutlineRow::new(
                    contents.into(),
                    index,
                    self.outline.drag.is_some(),
                    self.outline.hover,
                    tokens,
                )
                .indicator(
                    indicator
                        .filter(|target| target.section == index)
                        .map(|target| target.after),
                ),
            );
        }
        if self.outline.sections.is_empty() {
            list = list.push(
                text("Headings will appear here.")
                    .size(12)
                    .color(tokens.muted),
            );
        }
        list.into()
    }

    pub(super) fn quick_jump_view(&self) -> Element<'_, AppMessage> {
        let tokens = self.resolved_theme.tokens;
        let input = text_input("Search headings…", &self.outline.query)
            .id(QUICK_JUMP)
            .on_input(|query| AppMessage::Outline(Message::Query(query)))
            .on_submit(AppMessage::Outline(Message::Accept))
            .padding(12)
            .style(styles::input(tokens));
        let mut results = column![].spacing(4);
        // Keep the selected result visible without building thousands of widgets.
        let start = self.outline.selected.saturating_sub(2);
        for (slot, &index) in self.outline.results.iter().enumerate().skip(start).take(5) {
            let section = &self.outline.sections[index];
            let context = section.parent.map_or_else(
                || format!("Heading {}", section.level),
                |parent| {
                    format!(
                        "H{} · {}",
                        section.level, self.outline.sections[parent].title
                    )
                },
            );
            results = results.push(
                button(
                    column![
                        container(
                            text(if section.title.is_empty() {
                                "Untitled heading"
                            } else {
                                &section.title
                            })
                            .size(16)
                            .wrapping(iced::widget::text::Wrapping::None)
                        )
                        .width(Fill)
                        .height(22)
                        .clip(true),
                        container(
                            text(context)
                                .size(11)
                                .color(tokens.muted)
                                .wrapping(iced::widget::text::Wrapping::None)
                        )
                        .width(Fill)
                        .height(16)
                        .clip(true)
                    ]
                    .spacing(3),
                )
                .width(Fill)
                .padding(9)
                .on_press(AppMessage::Outline(Message::Jump(index)))
                .style(styles::button_style(
                    tokens,
                    if slot == self.outline.selected {
                        Button::Selected
                    } else {
                        Button::Quiet
                    },
                )),
            );
        }
        if self.outline.results.is_empty() {
            results = results.push(
                text(if self.outline.sections.is_empty() {
                    "Add a heading to get started."
                } else {
                    "No matching headings."
                })
                .size(14)
                .color(tokens.muted),
            );
        }
        container(
            column![
                row![
                    text("Quick jump").size(22),
                    space::horizontal(),
                    button("Close")
                        .on_press(AppMessage::CloseOverlay)
                        .style(styles::button_style(tokens, Button::Quiet))
                ]
                .align_y(Alignment::Center),
                input,
                results,
                text(format!(
                    "↑ ↓ to choose · Enter to jump · Esc to cancel   {} / {}",
                    if self.outline.results.is_empty() {
                        0
                    } else {
                        self.outline.selected + 1
                    },
                    self.outline.results.len()
                ))
                .size(11)
                .color(tokens.muted)
            ]
            .spacing(12),
        )
        .padding(24)
        .width(540)
        .style(styles::card(tokens))
        .into()
    }
}
