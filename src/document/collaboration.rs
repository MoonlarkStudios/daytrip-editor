//! Yrs ownership, UTF-8 transactions, stable intent, and origin-scoped history.
//!
//! Only `Document` commits edits. Transactions contain sorted, non-overlapping
//! ranges in the ORIGINAL source and are applied from right to left. CRDT deltas
//! use that same coordinate system; caches consume them only after commit.

use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ropey::Rope;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;
use yrs::types::Delta;
use yrs::undo::UndoManager;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{
    Any, Assoc, Doc, GetString, IndexedSequence, Observable, OffsetKind, Out, ReadTxn, StateVector,
    StickyIndex, Text, TextRef, Transact, Update,
};

use crate::document::selection::Selection;

pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_TRANSACTION_EDITS: usize = 65_536;
pub const MAX_STATE_BYTES: usize = 64 * 1024 * 1024;
const ROOT: &str = "markdown";

#[derive(Debug, Clone, Error)]
pub enum CollaborationError {
    #[error("Invalid UTF-8 edit range or overlapping transaction edits.")]
    InvalidRange,
    #[error(
        "This operation exceeds Daytrip’s text, batch, or retained collaboration limits. See docs/collaboration.md."
    )]
    TooLarge,
    #[error("Invalid or unsupported collaboration data: {0}")]
    InvalidData(String),
    #[error("This update belongs to a different document.")]
    Identity,
    #[error("The agent proposal conflicts with intervening edits: {0}")]
    Conflict(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEdit {
    pub range: Range<usize>,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOrigin {
    Human,
    Agent,
}
impl EditOrigin {
    fn key(self) -> &'static str {
        match self {
            Self::Human => "daytrip/human",
            Self::Agent => "daytrip/agent",
        }
    }
}

/// Association is independent of source/display and visual wrap affinity.
/// `Before` stays to the left of concurrent insertion at the gap; `After` moves
/// to its right. Deleted targets collapse to the surviving gap (Yrs semantics).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StablePosition {
    document: Uuid,
    index: StickyIndex,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StableSelection {
    forward: bool,
    anchor: StablePosition,
    head: StablePosition,
}

/// Preedit target is local. A changed replacement span collapses to its head,
/// so a later IME commit cannot delete newly arrived remote content.
#[derive(Debug, Clone)]
pub struct CompositionTarget {
    owner: u64,
    selection: StableSelection,
    hash: blake3::Hash,
    length: usize,
}

#[derive(Debug, Clone, Default)]
struct HistorySelection {
    before: Option<HistoryTarget>,
    after: Option<HistoryTarget>,
    removed_bytes: usize,
    inserted_bytes: usize,
    removed_units: usize,
    inserted_units: usize,
    operations: usize,
    author: Option<String>,
}

// Yrs 0.28 can lose an intra-item position when an entire deleted item is
// redone. Outside anchors plus a content hash restore selection endpoints in
// that case. This metadata never reverses text or authorizes an edit.
#[derive(Debug, Clone)]
struct HistoryTarget {
    stable: StableSelection,
    regions: Vec<HistoryRegion>,
}
#[derive(Debug, Clone)]
struct HistoryRegion {
    start: StablePosition,
    end: StablePosition,
    hash: blake3::Hash,
    length: usize,
    anchor: Option<usize>,
    head: Option<usize>,
}

/// A full encoded STATE is a mergeable bootstrap, including pending updates.
/// This is deliberately not a Yrs historical Snapshot restored at an old clock.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicaState {
    pub document: Uuid,
    #[serde(with = "encoded_update")]
    pub update: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSnapshot {
    pub state: ReplicaState,
    pub version: String,
    pub source: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedEdit {
    pub start: usize,
    pub end: usize,
    pub expected: String,
    pub replacement: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProposal {
    pub base: AgentSnapshot,
    pub author: String,
    pub edits: Vec<ProposedEdit>,
}

#[derive(Debug, Default)]
struct Observed {
    edits: Vec<SourceEdit>,
    invalid: bool,
}

#[derive(Default)]
struct PositionCache {
    generation: u64,
    entries: Vec<(usize, StickyIndex, bool)>,
}
impl PositionCache {
    fn current(&mut self, generation: u64) {
        if self.generation != generation {
            self.entries.clear();
            self.generation = generation;
        }
    }
    fn insert(&mut self, offset: usize, index: StickyIndex, captured: bool) {
        if self.entries.len() == 32 {
            self.entries.remove(0);
        }
        self.entries.push((offset, index, captured));
    }
}

type HistoryChange = (Vec<SourceEdit>, Option<Selection>);

pub struct Collaboration {
    id: Uuid,
    generation: u64,
    doc: Doc,
    text: TextRef,
    index: Rope,
    human: UndoManager<HistorySelection>,
    agent: UndoManager<HistorySelection>,
    history_context: Arc<Mutex<HistorySelection>>,
    observed: Arc<Mutex<Observed>>,
    // Conservative admission accounting, reset to encoded size on reopen.
    retained_bytes: usize,
    retained_structures: usize,
    agent_author: Option<String>,
    positions: Mutex<PositionCache>,
    last_edit: Option<(Instant, EditOrigin)>,
}
impl std::fmt::Debug for Collaboration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Collaboration")
            .field("document", &self.id)
            .field("replica", &self.doc.client_id())
            .finish_non_exhaustive()
    }
}

fn new_doc(id: Uuid) -> Doc {
    // Fresh OS-random identity on EVERY open/fork, never persisted as a writer ID.
    let bytes = *blake3::hash(Uuid::new_v4().as_bytes()).as_bytes();
    let client = u64::from_le_bytes(bytes[..8].try_into().expect("eight bytes")) & ((1 << 53) - 1);
    Doc::with_options(yrs::Options {
        guid: id.to_string().into(),
        client_id: yrs::ClientID::new(client),
        offset_kind: OffsetKind::Utf16,
        skip_gc: true,
        ..Default::default()
    })
}

fn observe(text: &TextRef) -> Arc<Mutex<Observed>> {
    let observed = Arc::new(Mutex::new(Observed::default()));
    let sink = Arc::clone(&observed);
    text.observe("daytrip/cache", move |txn, event| {
        let mut sink = sink
            .lock()
            .expect("text observer never panics while holding its lock");
        let mut offset = 0;
        for delta in event.delta(txn) {
            match delta {
                Delta::Retain(len, attrs) => {
                    offset += *len as usize;
                    sink.invalid |= attrs.is_some();
                }
                Delta::Deleted(len) => {
                    if let Some(last) = sink
                        .edits
                        .last_mut()
                        .filter(|edit| edit.range.end == offset)
                    {
                        last.range.end += *len as usize;
                    } else {
                        sink.edits.push(SourceEdit {
                            range: offset..offset + *len as usize,
                            text: String::new(),
                        });
                    }
                    offset += *len as usize;
                }
                Delta::Inserted(Out::Any(Any::String(value)), None) => {
                    if let Some(last) = sink
                        .edits
                        .last_mut()
                        .filter(|edit| edit.range.end == offset)
                    {
                        last.text.push_str(value);
                    } else {
                        sink.edits.push(SourceEdit {
                            range: offset..offset,
                            text: value.to_string(),
                        });
                    }
                }
                _ => sink.invalid = true,
            }
        }
    });
    observed
}

impl Collaboration {
    pub fn new(source: &str) -> Self {
        let id = Uuid::new_v4();
        let doc = new_doc(id);
        let text = doc.get_or_insert_text(ROOT);
        text.insert(&mut doc.transact_mut_with("daytrip/bootstrap"), 0, source);
        Self::attach(id, doc, text, source, source.len())
    }
    fn attach(id: Uuid, doc: Doc, text: TextRef, source: &str, retained_bytes: usize) -> Self {
        let history_context = Arc::new(Mutex::new(HistorySelection::default()));
        let manager = |origin: EditOrigin| {
            let mut manager = UndoManager::with_options(yrs::undo::Options {
                capture_timeout_millis: u64::MAX,
                ..Default::default()
            });
            manager.expand_scope(&doc, &text);
            manager.include_origin(origin.key());
            let context = Arc::clone(&history_context);
            manager.observe_item_added("selection", move |_, event| {
                *event.meta_mut() = context.lock().expect("history context lock").clone();
            });
            let context = Arc::clone(&history_context);
            manager.observe_item_updated("selection", move |_, event| {
                let context = context.lock().expect("history context lock");
                let meta = event.meta_mut();
                if meta.before.is_none() {
                    meta.before = context.before.clone();
                }
                meta.after = context.after.clone();
                meta.removed_bytes = meta.removed_bytes.saturating_add(context.removed_bytes);
                meta.inserted_bytes = meta.inserted_bytes.saturating_add(context.inserted_bytes);
                meta.removed_units += context.removed_units;
                meta.inserted_units += context.inserted_units;
                meta.operations += context.operations;
            });
            manager
        };
        let human = manager(EditOrigin::Human);
        let agent = manager(EditOrigin::Agent);
        let observed = observe(&text);
        Self {
            id,
            generation: 0,
            doc,
            text,
            index: Rope::from_str(source),
            human,
            agent,
            history_context,
            observed,
            retained_bytes,
            retained_structures: 2,
            agent_author: None,
            positions: Mutex::new(PositionCache::default()),
            last_edit: None,
        }
    }
    pub fn from_state(state: &ReplicaState) -> Result<(Self, String), CollaborationError> {
        let (doc, text) = decode_state(state)?;
        if text.diff(&doc.transact(), |_| ()).iter().any(|diff| {
            !matches!(&diff.insert, Out::Any(Any::String(_))) || diff.attributes.is_some()
        }) {
            return Err(CollaborationError::InvalidData(
                "non-text checkpoint".into(),
            ));
        }
        let source = text.get_string(&doc.transact()); // explicit load, never a keystroke
        if source.len() > MAX_SOURCE_BYTES {
            return Err(CollaborationError::TooLarge);
        }
        let mut collaboration =
            Self::attach(state.document, doc, text, &source, state.update.len());
        collaboration.retained_structures =
            crate::document::collaboration_wire::structure_count(&state.update)?;
        Ok((collaboration, source))
    }
    pub fn attribute_agent(&mut self, author: &str) {
        self.agent_author = Some(author.to_owned());
    }
    pub fn last_agent_author(&self) -> Option<&str> {
        self.agent
            .undo_stack()
            .last()
            .and_then(|item| item.meta.author.as_deref())
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn replica_id(&self) -> u64 {
        self.doc.client_id().get()
    }
    pub fn state(&self) -> ReplicaState {
        ReplicaState {
            document: self.id,
            update: self
                .doc
                .transact()
                .encode_state_as_update_v1(&StateVector::default()),
        }
    }
    pub fn position(&self, offset: usize, assoc: Assoc) -> StablePosition {
        let mut cache = self.positions.lock().expect("position cache lock");
        cache.current(self.generation);
        if let Some((_, index, _)) = cache
            .entries
            .iter()
            .find(|(byte, index, captured)| *captured && *byte == offset && index.assoc == assoc)
        {
            return StablePosition {
                document: self.id,
                index: index.clone(),
            };
        }
        let index = sticky(
            &self.text,
            &self.doc.transact(),
            units(&self.index, offset),
            assoc,
        );
        cache.insert(offset, index.clone(), true);
        StablePosition {
            document: self.id,
            index,
        }
    }
    pub fn resolve(&self, position: &StablePosition) -> Option<usize> {
        if position.document != self.id {
            return None;
        }
        let mut cache = self.positions.lock().expect("position cache lock");
        cache.current(self.generation);
        if let Some((offset, _, _)) = cache
            .entries
            .iter()
            .find(|(_, index, _)| *index == position.index)
        {
            return Some(*offset);
        }
        let offset = position
            .index
            .get_offset(&self.doc.transact())
            .map(|offset| bytes(&self.index, offset.index as usize))?;
        cache.insert(offset, position.index.clone(), false);
        Some(offset)
    }
    pub fn selection(&self, selection: Selection) -> StableSelection {
        // Carets follow inserted text. Range boundaries exclude new text at the
        // outside edge, preserving direction and the originally selected intent.
        let forward = selection.anchor <= selection.head;
        StableSelection {
            forward,
            anchor: self.position(
                selection.anchor,
                if forward { Assoc::After } else { Assoc::Before },
            ),
            head: self.position(
                selection.head,
                if forward && !selection.is_caret() {
                    Assoc::Before
                } else {
                    Assoc::After
                },
            ),
        }
    }
    pub fn resolve_selection(&self, selection: &StableSelection) -> Option<Selection> {
        let anchor = self.resolve(&selection.anchor)?;
        let head = self.resolve(&selection.head)?;
        Some(
            if selection.forward && head < anchor || !selection.forward && head > anchor {
                Selection::caret(head)
            } else {
                Selection { anchor, head }
            },
        )
    }
    pub fn composition_target(&self, selection: Selection) -> CompositionTarget {
        let mut hash = blake3::Hasher::new();
        for chunk in self.index.byte_slice(selection.range()).chunks() {
            hash.update(chunk.as_bytes());
        }
        CompositionTarget {
            owner: self.replica_id(),
            selection: self.selection(selection),
            hash: hash.finalize(),
            length: selection.range().len(),
        }
    }
    pub fn resolve_composition(&self, target: &CompositionTarget) -> Option<Selection> {
        if target.owner != self.replica_id() {
            return None;
        }
        let selection = self.resolve_selection(&target.selection)?;
        let mut hash = blake3::Hasher::new();
        for chunk in self.index.byte_slice(selection.range()).chunks() {
            hash.update(chunk.as_bytes());
        }
        Some(
            if selection.range().len() == target.length && hash.finalize() == target.hash {
                selection
            } else {
                Selection::caret(selection.head)
            },
        )
    }
    pub fn reset_group(&mut self) {
        self.last_edit = None;
        self.human.reset();
        self.agent.reset();
    }
    fn manager(&self, origin: EditOrigin) -> &UndoManager<HistorySelection> {
        match origin {
            EditOrigin::Human => &self.human,
            EditOrigin::Agent => &self.agent,
        }
    }
    pub fn can_undo(&self, origin: EditOrigin) -> bool {
        self.manager(origin).can_undo()
    }
    pub fn can_redo(&self, origin: EditOrigin) -> bool {
        self.manager(origin).can_redo()
    }

    pub fn commit(
        &mut self,
        source: &str,
        edits: &[SourceEdit],
        origin: EditOrigin,
        selection: Option<(Selection, Selection)>,
        separate: bool,
    ) -> Result<Vec<SourceEdit>, CollaborationError> {
        validate_edits(source, edits)?;
        if let Some((before, _)) = selection
            && (source.get(..before.anchor).is_none() || source.get(..before.head).is_none())
        {
            return Err(CollaborationError::InvalidRange);
        }
        let cost: usize = edits
            .iter()
            .map(|edit| edit.text.len().saturating_mul(2).saturating_add(256))
            .sum();
        let estimated_bytes = self.retained_bytes.saturating_add(cost);
        let estimated_structures = self.retained_structures.saturating_add(edits.len() * 6);
        let large = edits.len() > 256
            || edits.iter().any(|edit| edit.range.len() > 64 * 1024)
            || estimated_bytes > MAX_STATE_BYTES
            || estimated_structures > crate::document::collaboration_wire::MAX_STRUCTURES;
        let staged_cost = if large {
            let state = self.state();
            let (candidate, text) = decode_state(&state)?;
            let mut txn = candidate.transact_mut_with("daytrip/admission");
            edit_text(&mut txn, &text, &self.index, edits);
            txn.commit();
            drop(txn);
            let encoded = candidate
                .transact()
                .encode_state_as_update_v1(&StateVector::default());
            let structures = crate::document::collaboration_wire::structure_count(&encoded)?;
            if encoded.len() > MAX_STATE_BYTES - 4096
                || structures > crate::document::collaboration_wire::MAX_STRUCTURES - 16
            {
                return Err(CollaborationError::TooLarge);
            }
            Some((encoded.len() + 4096, structures + 16))
        } else {
            None
        };
        if edits.is_empty() {
            return Ok(Vec::new());
        }
        let now = Instant::now();
        let coalesce = !separate
            && self.last_edit.is_some_and(|(last, previous)| {
                previous == origin && now.duration_since(last) < Duration::from_millis(650)
            });
        if !coalesce {
            self.reset_group();
        }
        let before = selection.filter(|_| !coalesce).map(|(before, _)| {
            let regions: Vec<_> = edits
                .iter()
                .map(|edit| (edit.range.clone(), &source[edit.range.clone()]))
                .collect();
            history_target(before, &regions, |offset, assoc| {
                self.position(offset, assoc)
            })
        });
        let mut next_index = self.index.clone();
        patch_index(&mut next_index, edits);
        if let Some((_, after)) = selection {
            for byte in [after.anchor, after.head] {
                if byte > next_index.len_bytes()
                    || next_index.char_to_byte(next_index.byte_to_char(byte)) != byte
                {
                    return Err(CollaborationError::InvalidRange);
                }
            }
        }
        let mut txn = self.doc.transact_mut_with(origin.key());
        edit_text(&mut txn, &self.text, &self.index, edits);
        let mut post_positions = PositionCache {
            generation: self.generation + 1,
            entries: Vec::new(),
        };
        let after = selection.map(|(_, after)| {
            let mut shift = 0isize;
            let regions: Vec<_> = edits
                .iter()
                .map(|edit| {
                    let start = edit
                        .range
                        .start
                        .checked_add_signed(shift)
                        .expect("validated transaction size");
                    shift += edit.text.len() as isize - edit.range.len() as isize;
                    (start..start + edit.text.len(), edit.text.as_str())
                })
                .collect();
            history_target(after, &regions, |offset, assoc| {
                let index = if let Some((_, index, _)) =
                    post_positions
                        .entries
                        .iter()
                        .find(|(byte, index, captured)| {
                            *captured && *byte == offset && index.assoc == assoc
                        }) {
                    index.clone()
                } else {
                    let index = sticky(&self.text, &txn, units(&next_index, offset), assoc);
                    post_positions.insert(offset, index.clone(), true);
                    index
                };
                StablePosition {
                    document: self.id,
                    index,
                }
            })
        });
        *self.history_context.lock().expect("history context lock") = HistorySelection {
            before,
            after,
            removed_bytes: edits.iter().map(|edit| edit.range.len()).sum(),
            inserted_bytes: edits.iter().map(|edit| edit.text.len()).sum(),
            removed_units: edits
                .iter()
                .map(|edit| {
                    units(&self.index, edit.range.end) - units(&self.index, edit.range.start)
                })
                .sum(),
            inserted_units: edits
                .iter()
                .map(|edit| edit.text.encode_utf16().count())
                .sum(),
            operations: edits.len(),
            author: if origin == EditOrigin::Agent {
                self.agent_author.take()
            } else {
                None
            },
        };
        txn.commit();
        drop(txn);
        if let Some((bytes, structures)) = staged_cost {
            self.retained_bytes = bytes;
            self.retained_structures = structures;
        } else {
            self.retained_bytes = estimated_bytes;
            self.retained_structures = estimated_structures;
        }
        self.generation += 1;
        *self.positions.lock().expect("position cache lock") = post_positions;
        if separate {
            self.reset_group();
        } else {
            self.last_edit = Some((now, origin));
        }
        self.take_edits()
    }
    pub fn history(
        &mut self,
        origin: EditOrigin,
        redo: bool,
    ) -> Result<Option<HistoryChange>, CollaborationError> {
        let stack = self.manager(origin);
        let Some(item) = (if redo {
            stack.redo_stack()
        } else {
            stack.undo_stack()
        })
        .last() else {
            return Ok(None);
        };
        let restored = if redo {
            item.meta.inserted_bytes
        } else {
            item.meta.removed_bytes
        };
        let visible = item
            .insertions()
            .diff(&self.doc.transact().snapshot().delete_set);
        let visible_units: usize = visible
            .iter()
            .flat_map(|(_, ranges)| ranges.iter())
            .map(|range| (range.end - range.start) as usize)
            .sum();
        let expected_units = if redo {
            item.meta.removed_units
        } else {
            item.meta.inserted_units
        };
        let removed_minimum = if visible_units == expected_units {
            if redo {
                item.meta.removed_bytes
            } else {
                item.meta.inserted_bytes
            }
        } else {
            visible_units
        };
        let structures = item.meta.operations.saturating_mul(6).saturating_add(6);
        // Admission is conservative: remotely deleted content may reduce the
        // actual restoration. Refuse before mutation rather than exceeding caps.
        let cost = restored.saturating_mul(2).saturating_add(512);
        if self
            .index
            .len_bytes()
            .saturating_sub(removed_minimum)
            .saturating_add(restored)
            > MAX_SOURCE_BYTES
            || self.retained_bytes.saturating_add(cost) > MAX_STATE_BYTES
            || self.retained_structures.saturating_add(structures)
                > crate::document::collaboration_wire::MAX_STRUCTURES
        {
            return Err(CollaborationError::TooLarge);
        }
        let result = self.history_unchecked(origin, redo);
        if result.is_some() {
            self.retained_bytes += cost;
            self.retained_structures += structures;
        }
        Ok(result)
    }
    fn history_unchecked(&mut self, origin: EditOrigin, redo: bool) -> Option<HistoryChange> {
        let manager = self.manager(origin);
        let meta = if redo {
            manager.redo_stack().last()?
        } else {
            manager.undo_stack().last()?
        }
        .meta
        .clone();
        *self.history_context.lock().expect("history context lock") = meta.clone();
        let manager = match origin {
            EditOrigin::Human => &mut self.human,
            EditOrigin::Agent => &mut self.agent,
        };
        let changed = if redo {
            manager.redo_blocking()
        } else {
            manager.undo_blocking()
        };
        self.reset_group();
        self.generation += u64::from(changed);
        if !changed {
            return None;
        }
        let changes = self.take_edits().expect("undo only emits plain text");
        let selection = if redo { meta.after } else { meta.before }
            .and_then(|selection| self.resolve_history(&selection));
        Some((changes, selection))
    }
    fn resolve_history(&self, target: &HistoryTarget) -> Option<Selection> {
        let mut selection = self.resolve_selection(&target.stable)?;
        for region in &target.regions {
            let (Some(start), Some(end)) = (self.resolve(&region.start), self.resolve(&region.end))
            else {
                continue;
            };
            let restored_end = start.saturating_add(region.length);
            if start > end
                || restored_end > end
                || restored_end > self.index.len_bytes()
                || self
                    .index
                    .char_to_byte(self.index.byte_to_char(restored_end))
                    != restored_end
            {
                continue;
            }
            let mut hash = blake3::Hasher::new();
            for chunk in self.index.byte_slice(start..restored_end).chunks() {
                hash.update(chunk.as_bytes());
            }
            if hash.finalize() == region.hash {
                if let Some(offset) = region.anchor {
                    selection.anchor = start + offset;
                }
                if let Some(offset) = region.head {
                    selection.head = start + offset;
                }
            }
        }
        Some(selection)
    }
    fn take_edits(&mut self) -> Result<Vec<SourceEdit>, CollaborationError> {
        let observed = std::mem::take(&mut *self.observed.lock().expect("text observer lock"));
        if observed.invalid {
            return Err(CollaborationError::InvalidData(
                "only plain Markdown text is supported".into(),
            ));
        }
        let edits = translate(&self.index, observed.edits)?;
        patch_index(&mut self.index, &edits);
        Ok(edits)
    }

    /// Offline ingress is staged in an isolated compatible store. A rejected
    /// update cannot partially mutate the authoritative Doc (Yrs has no rollback).
    /// No source extraction occurs: validate the candidate's emitted deltas.
    pub fn receive(
        &mut self,
        source: &str,
        state: &ReplicaState,
    ) -> Result<Vec<SourceEdit>, CollaborationError> {
        if state.document != self.id {
            return Err(CollaborationError::Identity);
        }
        if state.update.len() > MAX_STATE_BYTES {
            return Err(CollaborationError::TooLarge);
        }
        // Validate the COMPLETE envelope before filtering known operations.
        // Update-level diffing retains delete sets and unknown/pending structs;
        // a transaction's integrated-state diff would lose pending operations.
        crate::document::collaboration_wire::validate(&state.update)?;
        let incoming = yrs::diff_updates_v1(
            &state.update,
            &self.doc.transact().state_vector().encode_v1(),
        )
        .map_err(|error| CollaborationError::InvalidData(error.to_string()))?;
        if incoming == [0, 0] {
            return Ok(Vec::new());
        }
        let current = self.state();
        if current.update.len().saturating_add(state.update.len()) > MAX_STATE_BYTES {
            return Err(CollaborationError::TooLarge);
        }
        let (candidate, text) = decode_state(&current)?;
        let observed = observe(&text);
        apply_encoded(&candidate, &incoming)?;
        validate_schema(&candidate, &text)?;
        let observed = observed.lock().expect("candidate observer lock");
        if observed.invalid {
            return Err(CollaborationError::InvalidData("non-text update".into()));
        }
        validate_edits(source, &translate(&self.index, observed.edits.clone())?)?;
        drop(observed);
        let candidate_state = candidate
            .transact()
            .encode_state_as_update_v1(&StateVector::default());
        if candidate_state == current.update {
            return Ok(Vec::new());
        }
        let structures = crate::document::collaboration_wire::structure_count(&candidate_state)?;
        if candidate_state.len() > MAX_STATE_BYTES {
            return Err(CollaborationError::TooLarge);
        }
        apply_encoded(&self.doc, &incoming)?;
        self.retained_structures = structures;
        self.retained_bytes = candidate_state.len();
        self.generation += 1;
        // Remote transactions are deliberately absent from both undo scopes.
        self.take_edits()
    }

    pub fn snapshot(&self, source: &str) -> AgentSnapshot {
        let state = self.state();
        AgentSnapshot {
            version: blake3::hash(&state.update).to_hex().to_string(),
            state,
            source: source.to_owned(),
        }
    }
    pub fn resolve_proposal(
        &self,
        source: &str,
        proposal: &AgentProposal,
    ) -> Result<Vec<SourceEdit>, CollaborationError> {
        if proposal.base.state.document != self.id {
            return Err(CollaborationError::Identity);
        }
        if proposal.edits.len() > 4096 || proposal.author.len() > 1024 {
            return Err(CollaborationError::TooLarge);
        }
        if blake3::hash(&proposal.base.state.update).to_hex().as_str() != proposal.base.version {
            return Err(CollaborationError::Conflict("base version digest".into()));
        }
        let (base, materialized) = Self::from_state(&proposal.base.state)?;
        if materialized != proposal.base.source {
            return Err(CollaborationError::Conflict("base source mismatch".into()));
        }
        if base.doc.transact().has_missing_updates() {
            return Err(CollaborationError::Conflict(
                "base has unresolved replica dependencies".into(),
            ));
        }
        let base_clock = base.doc.transact().snapshot();
        let current_clock = self.doc.transact().snapshot();
        if !matches!(
            current_clock.state_map.partial_cmp(&base_clock.state_map),
            Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater)
        ) || !base_clock
            .delete_set
            .diff(&current_clock.delete_set)
            .is_empty()
        {
            return Err(CollaborationError::Conflict(
                "base is not an accepted ancestor".into(),
            ));
        }
        let mut edits = Vec::new();
        for edit in &proposal.edits {
            if edit.replacement.len() > MAX_SOURCE_BYTES {
                return Err(CollaborationError::TooLarge);
            }
            let range = edit.start..edit.end;
            if materialized.get(range.clone()) != Some(edit.expected.as_str()) {
                return Err(CollaborationError::Conflict("expected base text".into()));
            }
            let resolve = |offset, assoc| {
                self.resolve(&base.position(offset, assoc))
                    .ok_or_else(|| CollaborationError::Conflict("deleted target".into()))
            };
            let start = resolve(edit.start, Assoc::After)?;
            let end = resolve(edit.end, Assoc::Before)?;
            if start > end || source.get(start..end) != Some(edit.expected.as_str()) {
                return Err(CollaborationError::Conflict("edited target".into()));
            }
            // An empty expected string says nothing about a gap. Validate both
            // surrounding scalar contexts, including insertions at either edge.
            if range.is_empty() {
                let left = materialized[..edit.start]
                    .char_indices()
                    .rev()
                    .nth(15)
                    .map_or(0, |(index, _)| index);
                let right = materialized[edit.end..]
                    .char_indices()
                    .nth(16)
                    .map_or(materialized.len(), |(index, _)| edit.end + index);
                let a = resolve(left, Assoc::Before)?;
                let b = resolve(right, Assoc::After)?;
                if a > b || source.get(a..b) != Some(&materialized[left..right]) {
                    return Err(CollaborationError::Conflict(
                        "insertion context changed".into(),
                    ));
                }
            }
            edits.push(SourceEdit {
                range: start..end,
                text: edit.replacement.clone(),
            });
        }
        edits.sort_by_key(|edit| edit.range.start);
        validate_edits(source, &edits)?;
        Ok(edits)
    }
    #[cfg(test)]
    pub fn doc_for_test(&self) -> &Doc {
        &self.doc
    }
    #[cfg(test)]
    pub fn diff_for_test(&self, clock: &StateVector) -> ReplicaState {
        ReplicaState {
            document: self.id,
            update: self.doc.transact().encode_diff_v1(clock),
        }
    }
    #[cfg(test)]
    pub fn materialize(&self) -> String {
        self.text.get_string(&self.doc.transact())
    }
}

fn edit_text(
    txn: &mut yrs::TransactionMut<'_>,
    text: &TextRef,
    index: &Rope,
    edits: &[SourceEdit],
) {
    for edit in edits.iter().rev() {
        let start = units(index, edit.range.start) as u32;
        if !edit.range.is_empty() {
            text.remove_range(
                txn,
                start,
                (units(index, edit.range.end) - start as usize) as u32,
            );
        }
        if !edit.text.is_empty() {
            text.insert(txn, start, &edit.text);
        }
    }
}

fn history_target(
    selection: Selection,
    regions: &[(Range<usize>, &str)],
    mut position: impl FnMut(usize, Assoc) -> StablePosition,
) -> HistoryTarget {
    let assoc = |offset| {
        for (range, _) in regions {
            if offset == range.start {
                return Assoc::Before;
            }
            if offset == range.end {
                return Assoc::After;
            }
        }
        Assoc::After
    };
    let stable = StableSelection {
        forward: selection.anchor <= selection.head,
        anchor: position(selection.anchor, assoc(selection.anchor)),
        head: position(selection.head, assoc(selection.head)),
    };
    let regions = regions
        .iter()
        .filter_map(|(range, source)| {
            let inside = |offset: usize| {
                (offset >= range.start && offset <= range.end).then(|| offset - range.start)
            };
            let anchor = inside(selection.anchor);
            let head = inside(selection.head);
            let interior = |offset: usize| offset > range.start && offset < range.end;
            if range.is_empty()
                || anchor.is_none() && head.is_none()
                || selection.is_caret() && !interior(selection.head)
            {
                return None;
            }
            Some(HistoryRegion {
                start: position(range.start, Assoc::After),
                end: position(range.end, Assoc::After),
                hash: blake3::hash(source.as_bytes()),
                length: source.len(),
                anchor,
                head,
            })
        })
        .collect();
    HistoryTarget { stable, regions }
}

fn units(index: &Rope, byte: usize) -> usize {
    index.char_to_utf16_cu(index.byte_to_char(byte))
}
fn bytes(index: &Rope, unit: usize) -> usize {
    index.char_to_byte(index.utf16_cu_to_char(unit))
}
fn patch_index(index: &mut Rope, edits: &[SourceEdit]) {
    for edit in edits.iter().rev() {
        let start = index.byte_to_char(edit.range.start);
        let end = index.byte_to_char(edit.range.end);
        index.remove(start..end);
        index.insert(start, &edit.text);
    }
}
fn translate(index: &Rope, edits: Vec<SourceEdit>) -> Result<Vec<SourceEdit>, CollaborationError> {
    edits
        .into_iter()
        .map(|edit| {
            if edit.range.end > index.len_utf16_cu() {
                return Err(CollaborationError::InvalidRange);
            }
            let start = bytes(index, edit.range.start);
            let end = bytes(index, edit.range.end);
            if units(index, start) != edit.range.start || units(index, end) != edit.range.end {
                return Err(CollaborationError::InvalidData(
                    "update splits a Unicode scalar".into(),
                ));
            }
            Ok(SourceEdit {
                range: start..end,
                text: edit.text,
            })
        })
        .collect()
}
fn sticky<T: ReadTxn>(text: &TextRef, txn: &T, offset: usize, assoc: Assoc) -> StickyIndex {
    if offset == text.len(txn) as usize && assoc == Assoc::After {
        StickyIndex::new(yrs::IndexScope::Root(ROOT.into()), assoc)
    } else {
        text.sticky_index(txn, offset as u32, assoc)
            .expect("current scalar boundary")
    }
}

pub fn validate_edits(source: &str, edits: &[SourceEdit]) -> Result<(), CollaborationError> {
    if edits.len() > MAX_TRANSACTION_EDITS {
        return Err(CollaborationError::TooLarge);
    }
    let mut previous: Option<Range<usize>> = None;
    let mut size = source.len();
    for edit in edits {
        if edit.range.start > edit.range.end
            || source.get(edit.range.clone()).is_none()
            || previous.as_ref().is_some_and(|range| {
                edit.range.start < range.end
                    || edit.range.start == range.end && (range.is_empty() || edit.range.is_empty())
            })
        {
            return Err(CollaborationError::InvalidRange);
        }
        previous = Some(edit.range.clone());
        size = size
            .checked_sub(edit.range.len())
            .and_then(|size| size.checked_add(edit.text.len()))
            .ok_or(CollaborationError::TooLarge)?;
    }
    if size > MAX_SOURCE_BYTES {
        return Err(CollaborationError::TooLarge);
    }
    Ok(())
}
fn apply_encoded(doc: &Doc, bytes: &[u8]) -> Result<(), CollaborationError> {
    if bytes.len() > MAX_STATE_BYTES {
        return Err(CollaborationError::TooLarge);
    }
    crate::document::collaboration_wire::validate(bytes)?;
    let update = Update::decode_v1(bytes)
        .map_err(|error| CollaborationError::InvalidData(error.to_string()))?;
    doc.transact_mut_with("daytrip/remote")
        .apply_update(update)
        .map_err(|error| CollaborationError::InvalidData(error.to_string()))
}
fn decode_state(state: &ReplicaState) -> Result<(Doc, TextRef), CollaborationError> {
    crate::document::collaboration_wire::validate(&state.update)?;
    let update = Update::decode_v1(&state.update)
        .map_err(|error| CollaborationError::InvalidData(error.to_string()))?;
    // Include clients from pending structs, not just the integrated state vector.
    let known = update.state_vector();
    let doc = loop {
        let doc = new_doc(state.document);
        if !known.contains_client(&doc.client_id())
            && update.delete_set().get(&doc.client_id()).is_none()
        {
            break doc;
        }
    };
    let text = doc.get_or_insert_text(ROOT);
    doc.transact_mut_with("daytrip/load")
        .apply_update(update)
        .map_err(|error| CollaborationError::InvalidData(error.to_string()))?;
    validate_schema(&doc, &text)?;
    Ok((doc, text))
}

fn validate_schema(doc: &Doc, text: &TextRef) -> Result<(), CollaborationError> {
    let txn = doc.transact();
    if txn
        .root_refs()
        .any(|(name, value)| name != ROOT || !matches!(value, Out::YText(_)))
    {
        return Err(CollaborationError::InvalidData("unknown root type".into()));
    }
    if text.len(&txn) as usize > MAX_SOURCE_BYTES {
        return Err(CollaborationError::TooLarge);
    }
    Ok(())
}

// JSON is only the agent interchange format; checkpoints use a binary frame.
mod encoded_update {
    use super::MAX_STATE_BYTES;
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        if encoded.len() > MAX_STATE_BYTES.div_ceil(3) * 4 {
            return Err(serde::de::Error::custom(
                "collaboration state exceeds 64 MiB",
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(serde::de::Error::custom)?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(serde::de::Error::custom(
                "collaboration state exceeds 64 MiB",
            ));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolved_deleted_positions_do_not_poison_new_position_capture() {
        let mut collaboration = Collaboration::new("abc");
        let old = collaboration.position(1, Assoc::After);
        collaboration
            .commit(
                "abc",
                &[SourceEdit {
                    range: 1..2,
                    text: String::new(),
                }],
                EditOrigin::Human,
                None,
                true,
            )
            .unwrap();
        assert_eq!(collaboration.resolve(&old), Some(1));
        let new = collaboration.position(1, Assoc::After);
        assert_ne!(new.index.id(), old.index.id());
    }
    #[test]
    fn text_and_history_admission_fail_without_partial_mutation() {
        let source = "base";
        let mut collaboration = Collaboration::new(source);
        collaboration.retained_bytes = MAX_STATE_BYTES;
        collaboration.retained_structures = crate::document::collaboration_wire::MAX_STRUCTURES;
        // A small edit can be safely re-admitted after conservative accounting
        // has reached its cap; staging measures its actual encoded state.
        collaboration
            .commit(
                source,
                &[SourceEdit {
                    range: 0..0,
                    text: "x".into(),
                }],
                EditOrigin::Human,
                None,
                true,
            )
            .unwrap();
        assert_eq!(collaboration.materialize(), "xbase");
        let huge = SourceEdit {
            range: 0..0,
            text: "x".repeat(MAX_SOURCE_BYTES + 1),
        };
        assert!(
            collaboration
                .commit("xbase", &[huge], EditOrigin::Human, None, true)
                .is_err()
        );
        assert_eq!(collaboration.materialize(), "xbase");
        collaboration.retained_bytes = MAX_STATE_BYTES;
        assert!(collaboration.history(EditOrigin::Human, false).is_err());
        assert_eq!(collaboration.materialize(), "xbase");
    }
}
