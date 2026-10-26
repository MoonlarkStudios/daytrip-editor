# Collaborative Markdown foundation

Dosty uses **Yrs 0.28.0**, pinned in Cargo, for one root `TextRef` named `markdown`. `Document` owns its `Collaboration`, source cache, and parsed blocks. Every editor source mutation commits to Yrs first. The String, UTF-16 coordinate index, projection, native content, and layout are derived state. Focus, disclosure, wrapping, selection geometry, and scrolling are local.

This delivery supports one human with accepted agent proposals and independent offline replicas. Networking, authentication, presence, and a branch-review interface are separate work.

## Library choice and indexing

[Yrs](https://docs.rs/yrs/0.28.0/yrs/) supplies shared text, permanent positions, selective undo, and compatible encoded state. [Automerge](https://automerge.org/docs/reference/documents/) has useful historical-document and branching facilities, but this delivery needs text editing and origin-scoped undo rather than a historical review interface. There is one backend.

Yrs text is explicitly configured for **UTF-16 code units**. Public editor ranges remain **UTF-8 byte offsets**. A derived Ropey index converts between them incrementally; it preserves exact text, including CRLF, LF, CR, combining characters, joined emoji, and RTL. Changes splitting a Unicode scalar are rejected. Human grapheme movement remains Iced's responsibility.

Using byte-indexed editing alone is insufficient for Yrs 0.28 permanent positions: its internal IDs use UTF-16 clocks. We therefore use one explicit coordinate bridge for editing, deltas, and positions. The pinned Yrs dependency patch reuses its existing cached UTF-16 item lengths during traversal and item merging; it changes no coordinates or encoded state. See [dependency provenance and removal policy](../vendor/README.md). A bounded, generation-scoped position cache avoids repeating those traversals for layout and selection. Resolved deleted positions are cached separately from newly captured positions, so a collapsed old target cannot become a new target accidentally. Measured costs and remaining traversal limits are recorded in [the UTF-16 performance report](utf16-performance.md).

## Transactions and ownership

`Document::transact` takes sorted, disjoint source edits. **All offsets refer to the transaction's original source.** Operations execute right to left inside one Yrs transaction. Adjacent nonempty ranges are allowed; insertions touching another edit's boundary and duplicate insertion points are rejected as ambiguous. Validate the whole batch and resulting size before mutation.

Typing, paste, deletion, formatting, Markdown repair, tables, image removal, find/replace, and programmatic edits use this path. Replace-all submits individual matches rather than replacing the whole source. Rich repair is already included in the originating source edit. Receivers apply committed operations directly and never run the local rich-edit repair path.

The text observer produces UTF-16 deltas, which are translated against the predecessor index and then applied from the end to both caches. No per-keystroke source extraction or document diff creates replicated operations. Single certified block updates retain the existing incremental parser/projection paths. Batches covering at most 32 certified independent block windows parse each affected block once (including multiple edits in one block). Projection preflights all windows and disclosure before publishing one immutable mapping successor and sparse native/layout regions. Uncertain boundaries, global dependencies, or more than 32 windows use the full scanner once and general projection path; CRDT operations remain granular. Equal-length batches invalidate changed blocks too.

Offline admission validates the full original wire envelope, then uses Yrs `diff_updates_v1` against the active integrated state vector. This retains delete sets and unknown/pending structs while excluding known insertions. It stages the filtered update in an isolated compatible Doc and applies the same operations to the existing active Doc only after all checks pass. Full persisted state still includes pending operations. An empty filtered update avoids staging; delete-only updates still enter validation. Retained admission accounting uses the measured candidate encoded size. See [offline imports](offline-imports.md) for limits and measurements.

Native synchronization uses proven projection regions where available. Conservative full projections use sparse physical-line differences; very large native replacements use deferred loading to avoid cosmic-text's quadratic line deletion. The rich widget survives these changes. Existing immutable mapping/highlight snapshots remain available to old layout until it finishes anchoring.

`Document::revision` and the collaboration owner's generation are local cache counters. Neither is a document identity or causal version. A versioned agent snapshot carries a digest of its complete encoded state, including deletions and pending updates. The two clocks in a Yrs historical snapshot are not a replacement for that state.

Incoming updates are schema-checked, then staged in an isolated compatible Yrs store because Yrs transactions cannot roll back. Only emitted text deltas update the materialized source; staging does not extract source text. Full encoded state is copied for admission, so offline import has document-size-dependent cost. Large local transactions also stage admission to measure retained state accurately. Local typing uses incremental admission accounting.

Yrs handles duplicate/out-of-order delivery. Identical admitted state does not advance local generations. There is no automatic outbound update observer: importing an update cannot echo it through another mutation path. Offline export is an explicit action.

## Stable intent, undo, and composition

Selection endpoints, drag anchors, viewport rows, visual wrap positions, composition targets, and proposal targets use Yrs `StickyIndex` positions. Resolve them to current byte offsets for existing projection/rendering. Parsed nodes and short-lived geometry continue using ordinary byte ranges.

`Before` remains left of an insertion at a gap; `After` follows it. Carets follow inserted text. Range boundaries exclude outside insertions and preserve anchor/head direction. Deleted targets collapse to a surviving gap. If endpoint resolution crosses the original direction after deletion/replacement, collapse the selection to its head instead of selecting newly arrived replacement content. Visual upstream/downstream wrap affinity remains separate from CRDT insertion affinity and source/display affinity.

The widget captures an anchor before source changes, resolves it against committed state, and restores its screen Y after reflow. Remote/proposal changes preserve an offscreen viewport rather than bringing the old caret into view. Drag anchors are cleared on release/unfocus; they cannot restart on a later update. Disclosure remains independent between replicas.

There are separate Yrs undo managers for **human** and **accepted-agent** origins. Remote imports and loading are excluded. The application controls 650 ms grouping with a monotonic clock; navigation and explicit commands break a group. A proposal is one agent group and records its author locally. Human undo/redo affects human operations even after agent or remote edits. Agent undo/redo affects accepted proposals while preserving the human's current stable selection. An imported independent branch is remote work; undo it on its originating replica and import the resulting update.

Fixed-offset inverse patches have been removed. History selection metadata uses stable positions. Yrs 0.28 can lose an intra-item offset when following a completely restored item; an outside-anchored, hashed restoration region repairs selection endpoints only when that text is actually restored. If intervening edits changed the region, use ordinary deleted-target affinity. This metadata never authorizes or reverses text.

IME preedit is transient widget state. The first nonempty preedit captures a stable replacement target and content hash; a direct commit captures the current target. A commit resolves that target and creates exactly one source transaction with its own undo group. Incoming changes before the target rebase it. If its replacement text changed or was deleted, retain preedit and collapse the eventual replacement to the resolved head, preserving incoming text. Closing composition or changing documents clears its target. The native Source widget delegates focus, preedit drawing, and input handling to Iced. A small wrapper replaces its commit message with the same stable-target transaction used by the rich editor; it does not add another paste. Reopening resets the wrapper and native composition state even when the CRDT document identity is unchanged.

Undo stacks, grouping, author metadata, and presentation state are **session-local**. They are not automatically encoded in Yrs updates or restored after reopening. This matches Dosty's existing session history behavior. Synchronization information and deleted payloads remain in persisted CRDT state.

## Human–agent workflow

Open **Appearance → Collaboration…**:

- **Export agent snapshot…** writes immutable JSON containing source, document ID, encoded base state, and version digest. Encoded update bytes use base64.
- **Accept agent proposal…** reads a JSON proposal and applies it only after validation. Choosing this action/file approves applying that proposal; there is no preview merge.
- **Export replica…** writes a binary `.dostyreplica` containing full compatible state. Existing `.koanreplica` and `.daytripreplica` files remain supported when opening or importing.
- **Open replica as offline copy…** opens that state with a fresh active writer identity, using the ordinary unsaved-document guard. Save a separate Markdown copy and export its approved state to merge later.
- **Merge approved replica…** admits a compatible replica; **Undo agent / Redo agent** operate on accepted local proposals.

A proposal has this shape, with `base` copied intact from the exported snapshot:

```json
{
  "base": { "state": { "document": "…", "update": "base64…" }, "version": "…", "source": "…" },
  "author": "Copy editor",
  "edits": [
    { "start": 10, "end": 15, "expected": "First", "replacement": "Better" }
  ]
}
```

Offsets are **UTF-8 bytes**, not JavaScript UTF-16 indices or Python string indices. Agents must compute byte offsets against `base.source` without normalizing line endings. For example, Python's `len(source[:character_index].encode("utf-8"))` converts a character index to bytes.

Validate the base digest, identity, materialized base source, integrated state-vector ancestry **and deletions**. Bases with unresolved dependencies must wait for synchronization. Create targets in that base's compatible store and resolve them in the current document without merging the base. Expected text must match both versions. A zero-width edit also validates up to 16 surrounding Unicode scalars on both sides; an empty expected string alone never validates a gap. An overlap, changed context, deleted target, invalid range, or wrong identity returns an explicit conflict without applying any part of the proposal. Unrelated edits outside those targets can proceed while an agent works.

A **mergeable fork** is a full encoded state loaded with a new writer identity. Independently seeding two documents with the same Markdown does not share history. A Yrs historical `Snapshot` is a clock/deletion marker for inspection; Dosty does not restore such a view as an editable fork or promise it has equivalent merge semantics. Exported full states are the supported bootstrap format.

## Persistence and recovery

For `note.md`, Dosty stores a hidden `.note.md.daytrip` checkpoint beside the physical Markdown target, plus a transient `.note.md.daytrip-journal` and an advisory `.note.md.daytrip-lock`. Symlinks resolve to their target. Preserve/copy the Markdown and checkpoint together to retain compatible history across cloning and offline use.

These storage names intentionally survive the Dosty rename to preserve existing
data and lock coordination; see [identity and compatibility](identity-migration.md).

A checkpoint has a versioned binary frame, UUID, length, BLAKE3 checksum, and complete Yrs update V1. The active writer ID is never stored as a reusable writer configuration. Each open/fork gets an OS-random-derived 53-bit identity excluded from the encoded state's known clients, including pending structs. Historical operation IDs remain unchanged. Document identity remains stable across reopen/copy; new ordinary Markdown imports get a new UUID.

Saving captures source and CRDT state from the same owner generation. Disk work runs off the UI thread. An exclusive file lock serializes cooperating writers. Compare the loaded/saved Markdown **and checkpoint version/identity** before publishing; another window cannot overwrite causal-only changes simply because Markdown text still matches. Edits made while a save runs remain dirty.

Publication order is:

1. Sync a journal containing predecessor Markdown/checkpoint hashes and the full successor checkpoint.
2. Atomically replace/sync Markdown using existing temporary-file, permissions, symlink, and directory-sync behavior.
3. Atomically replace/sync the checkpoint.
4. Remove the journal and sync its directory on Unix.

On reopening under an exclusive lock, recovery completes the recorded save only if both files match recorded predecessors/successors. Otherwise preserve all files and report a recovery conflict. A checkpoint/source mismatch, damaged metadata, a missing checkpoint beside a known local lock file, wrong identity on save/import, or external Markdown modification is explicit; loading never silently seeds a new collaboration history over stale metadata. Ordinary read-only Markdown opens do not need directory write permission. Non-cooperating external writers still have the usual check/rename race; advisory locks cannot force another editor to cooperate.

To import intentionally edited external Markdown as a **new document**, preserve the original pair, then open a separate Markdown copy without its checkpoint. This explicitly starts a new identity; it does not reconnect old replicas automatically.

## Bounds and retention

- Markdown/source: **16 MiB**, exact UTF-8, no truncation.
- Encoded collaboration state/update: **64 MiB**; ingress additionally bounds combined predecessor/input size before staging.
- Encoded retained structures/ranges/clients: **200,000**.
- Source transaction: **65,536** disjoint edits; agent proposal: **4,096** edits and a 1,024-byte author label.
- Agent JSON file: **160 MiB**; unknown fields/unsupported schema, roots, formatting attributes, embedded values, nested types, malformed counts/IDs, and trailing binary data are rejected.

Wire preflight bounds allocations before invoking Yrs decoding, including pending operations. UTF-16 deltas splitting a scalar are rejected before live mutation. Never selectively drop operations to fit a limit.

Yrs garbage collection is disabled. Canonical checkpoint encoding merges representation where Yrs permits, retains pending updates, and keeps the information needed for offline synchronization. Deleted text remains in checkpoints. No automatic history pruning, tombstone removal, causal reset, or lossy compaction occurs. Admission and undo-size accounting are conservative: near retained-state/text limits an operation can be refused even if a more expensive proof might admit it. Preserve/export the state and Markdown; starting a new history is an explicit import under a new identity, not transparent compaction. Network-safe compaction and durable historical review are future work.

## Validation

The regression suite exercises the real application/native buffer/widget as well as the document model: shared-state three-replica convergence, concurrent replacements, delayed/duplicate/out-of-order delivery, pending-state reopen, deterministic randomized edits, byte/UTF-16 conversion, direction/affinity, local disclosure, origin history, active composition, proposal conflicts, batch render invalidation, cooperative file locks, interrupted publication, symlinks/permissions, and malformed/oversized input. Full materialization and fresh parsing/mapping comparisons are test oracles outside timed interaction paths.

Run `mise run check`, `mise run release`, `mise run bench`, and `mise run profile`; the additional ignored `large_document_collaboration` test measures remote batch interactions. See the [offline import measurements](offline-imports.md) and [later writing-workflow comparisons](writing-workflows.md#offline-imports-retained-state-and-memory). Actual OS CJK IME delivery, GPU presentation, accessibility, and Windows/Linux filesystem durability still need manual platform validation.
