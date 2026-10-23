# macOS clipboard, drop, and input-method validation

Validated on 2026-10-23 with macOS 26.7.1 (25G241), Apple silicon, mise Rust 1.95.0 and Iced 0.14. The starting commit was `20828c3`. Native smoke testing used that existing release bundle, a separate `STILL_CONFIG_DIR`, and a temporary copy of **The quiet hour**. Existing documents were not closed or reloaded. The scratch document was saved before terminating only its identified Still process.

## Fixed composition lifetime

Clearing preedit does not necessarily close the input method. Previously, canceling composition, moving the selection, and starting another composition could reuse the canceled target and replace the wrong text. A regression reproduced this before the fix.

Live and Source editors now capture a fresh stable target at the start of each nonempty preedit run. A subsequent local keyboard/pointer action also retires a cleared target. An empty preedit alone retains the pending target because macOS can clear marked text immediately before Commit. This preserves the original stable replacement through intervening remote edits. Opening/closing the input method and changing document sessions still reset composition state. Preedit remains transient; each commit uses the existing single CRDT transaction and undo group.

This distinction was checked against the locked Winit macOS implementation: `unmark_text` clears preedit without disabling IME, while `insert_text` queues empty preedit followed by Commit. The tests retain both sequences.

The Source wrapper tracks whether preedit is nonempty because the standard Iced widget owns its private presentation state. No native widget is replaced, and the CRDT, selection affinity, and undo algorithms are unchanged.

## What was exercised

| Workflow | Native macOS observation | Automated final implementation |
| --- | --- | --- |
| Multiline text paste | Combining accents, joined emoji, Arabic, Japanese, Chinese, Korean saved with exact source bytes | Existing Unicode/native-buffer/CRDT tests retained |
| URL over selected prose | Created a Markdown link; Undo restored the phrase | Separate link/IME undo groups retained |
| Finder copied image file | Imported once into a hashed document asset; Undo/Redo restored it | Actual asynchronous importer task, immutable bytes, source/projection/native oracles |
| Preview raster clipboard | Imported once as PNG with a distinct content hash | Existing codec/resource-limit tests retained |
| Whole-document Cut/Paste | Included headings, tables, image references, credits and Unicode; saved result byte-identical to the 2,446-byte input | Existing cross-block selection/copy/undo tests retained |
| Native marked text | U.S. Option+E preedit and accented-character commit; Undo restored heading syntax/layout | Japanese/Chinese/Korean cancel/move/recompose and direct-commit cases in both editors |
| HTML save panel | Completed export; 12,531,476 bytes, three embedded images, Unicode retained | Existing export and representative-document round-trip tests retained |
| File drag/drop delivery | **Unverified**: desktop work became active before a native drop was completed | App drop routing and real background image import tested, including a later reverse selection and intervening replica edit |
| Unsaved-document guards | Native panel interaction left for a quiet desktop | Image drop → first Save cancellation and document drop → Cancel preserve source, reverse selection, dirty state and Undo |
| CJK candidate windows | **Unverified**: only U.S. input was installed; user requested no keyboard setting changes | Real Iced preedit/clear/commit events with 日本語, 中文 and 한국어; source/cache equivalence and Undo/Redo |

Native testing stopped when the user confirmed other desktop work was active. No further UI input was sent. Keyboard settings were left unchanged. The composition fix was subsequently validated through the actual Live/Source widgets and application model, **not** through a post-fix native candidate-window session. Glyph appearance and candidate-window placement for installed CJK input methods still need that session; text paste is not evidence of candidate-window correctness.

`iced_runtime` is a development-only direct dependency, already present transitively, so the drop test can poll the application's real import Task and deliver its result. It does not run file choosers, change the OS clipboard, write user preferences, or expose production internals. Missing-image tasks are excluded from this particular test; ordinary application tests still cover image loading.

## Repeat the remaining platform checks

Use a temporary configuration and a saved copy of the bundled sample. Keep existing windows open. A drop currently inserts/replaces at the source selection, as documented in [writing workflows](writing-workflows.md); pointer-based drop placement is not introduced here.

1. Drop one PNG/JPEG/GIF/WebP from Finder. Verify one figure, durable local asset, Cmd+Z/Cmd+Shift+Z, and continued keyboard selection across adjacent blocks. Repeat while a reverse selection is active.
2. Drop an image into an unsaved document, cancel first Save, and verify selection, source and Undo. Drop a Markdown file over unsaved writing, cancel the document-change guard, and verify the same invariants.
3. With an already enabled Japanese input method, compose `nihongo`, inspect candidates, choose 日本語 and commit. Repeat Chinese `zhongwen` → 中文 and Korean two-set `gksrmf` → 한글. Test heading/body/table-cell targets, replacement of forward/reverse selections, cancellation followed by moving and recomposing, and one-step Undo/Redo.
4. Check candidate placement at wrapped lines and near the viewport bottom in both Live and Source modes. Confirm no duplicate insertion, stale replacement, lost focus or unexplained scroll jump. Leave input-source settings as found.

## Automated evidence

Run serially from the repository root:

```sh
mise run check
mise run release
STILL_PROFILE_BLOCKS=10000 STILL_PROFILE_SAMPLES=30 mise run profile
STILL_PROFILE_BLOCKS=50000 STILL_PROFILE_SAMPLES=30 mise run profile
```

The profiles exercise keyboard input, disclosure, selection/dragging, scrolling, IME, Undo/Redo and derived-state oracles through software rendering. They exclude OS delivery, candidate windows and GPU presentation. Other desktop work was active, so these are regression runs, not a controlled performance comparison. No speedup or input-to-photon claim is made.

Final validation passed: **156 ordinary tests**, formatting, Clippy with warnings denied for application targets, release build, and both interaction profiles. The initial sandboxed check could not bind the HTTP image test's localhost listener; the complete check passed with localhost access. Existing vendored Yrs warnings remain outside this change.

The [macOS bundle build and signature verification](validation/native-writing/bundle.txt) also passed. Replacing the executable uses the existing atomic bundle script; no existing window was restarted, and no post-fix UI session was attempted while other desktop work was active.

| Profile observation | 10k blocks | 50k blocks |
| --- | ---: | ---: |
| Cold App/source initialization, ms (one sample) | 155.99 | 715.53 |
| Middle typing, ms p50 / p95 | 2.076 / 3.268 | 5.891 / 7.895 |
| Disclosure, ms p50 / p95 | 3.118 / 3.641 | 16.046 / 17.076 |
| Half-document selection, ms p50 / p95 | 1.571 / 1.744 | 2.691 / 3.135 |
| Scrolling, ms p50 / p95 | 1.521 / 1.719 | 1.587 / 1.849 |
| IME commit, ms (one sample) | 3.002 | 7.208 |

The fixtures remain 910,450 / 4,574,450 source bytes with a 720×800 software viewport and 30 warm samples. Full phase breakdowns, tails and history interactions are in the raw logs: [check](validation/native-writing/check.txt), [release](validation/native-writing/release.txt), [10k](validation/native-writing/profile-10k.txt), [50k](validation/native-writing/profile-50k.txt), [environment and source hashes](validation/native-writing/environment.json). Existing document-storage growth and large-import limitations remain as recorded in [writing workflows](writing-workflows.md).
