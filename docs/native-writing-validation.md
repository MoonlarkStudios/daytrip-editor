# macOS clipboard, drop, and input-method validation

Validated on 2026-10-23 with macOS 26.7.1 (25G241), Apple silicon, mise Rust 1.95.0 and Iced 0.14. The starting commit was `e73e4c0`; the native PNG drop follow-up used `901cc60`. Both sessions used separate `DAYTRIP_CONFIG_DIR` directories and temporary copies of **The quiet hour**. Existing documents were not closed or reloaded. The initial scratch process was terminated only after saving; the follow-up document was left saved and open.

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
| File drag/drop delivery | **PNG passed** in the follow-up below: one insertion, exact asset bytes, separate details/drop Undo, exact Redo, forward/reverse cross-block cut/Undo | App drop routing and real background image import tested, including a later reverse selection and intervening replica edit |
| Unsaved-document guards | New-document prompt and Escape cancellation passed with a temporary typed character; the document stayed open and Undo removed only that character | Image drop → first Save cancellation and document drop → Cancel preserve source, reverse selection, dirty state and Undo |
| CJK candidate windows | **Unverified**: only U.S. input was installed; user requested no keyboard setting changes | Real Iced preedit/clear/commit events with 日本語, 中文 and 한국어; source/cache equivalence and Undo/Redo |

Initial native testing stopped when the user confirmed other desktop work was active, and resumed only after the user said the desktop was available. Keyboard settings were left unchanged throughout. The composition fix was validated through the actual Live/Source widgets and application model, **not** through a post-fix native CJK candidate-window session. Glyph appearance and candidate-window placement for installed CJK input methods still need that session; text paste is not evidence of candidate-window correctness.

`iced_runtime` is a development-only direct dependency, already present transitively, so the drop test can poll the application's real import Task and deliver its result. It does not run file choosers, change the OS clipboard, write user preferences, or expose production internals. Missing-image tasks are excluded from this particular test; ordinary application tests still cover image loading.

## Native PNG drop follow-up

The follow-up ran the `901cc60` release executable in a separate, ad-hoc-signed **Daytrip Drop Validation** app with bundle identifier `app.daytrip.editor.drop-validation`, its own configuration, and a temporary `Drop workflow.md`. Only the new test windows were arranged. The other Finder windows and existing writing sessions were preserved.

The automation successfully moved a disposable PNG into a disposable folder within Finder, but its cross-application drag attempts did not produce a Daytrip insertion. The user then performed the Finder-to-Daytrip PNG gesture and confirmed release. The assistant verified the resulting native UI and files; this result is not attributed to a successful automated cross-application gesture.

- The live editor displayed one new photograph at the existing source caret. The original Markdown remained an exact prefix, with two image references total (the sample image and the drop).
- Exactly one asset was published in `Drop workflow.assets/`. Its bytes matched the input PNG exactly. The caption and credit the user added were saved as ordinary Markdown.
- One Cmd+Z removed the details edit while retaining the image. A second Cmd+Z removed the drop, restoring the entire original sample byte for byte. The asset remained available for Redo.
- Two Cmd+Shift+Z operations restored the full 2,221-byte document, including the caption and credit, byte for byte.
- In both selection directions, Cmd+Shift+Up/Down selected across headings, prose, the table, and both images. Cut displayed an empty editor; Undo and Save restored those same 2,221 source bytes.
- After typing one temporary character, Cmd+N showed the unsaved-document guard. Escape kept the same document and character. Undo removed only that character, and Save again matched the full post-drop source exactly.

No application change was needed for this native PNG path. The final source SHA-256 was `f52410609ecb067d360d0775ba746ffb852dcdd01530a98e54cd1957852a1b98`. [Structured evidence](validation/native-writing/native-drop.json) records the executable/input/source hashes and comparisons. The follow-up `mise run check` passed all 156 ordinary tests; its [check output](validation/native-writing/native-drop-check.txt) is retained. The saved test document was left open with the user's caption and credit intact.

The native gesture exercised PNG insertion at a caret. JPEG/GIF/WebP gestures, a drop over an already active reverse selection, image-drop first-save cancellation, and Markdown-file-drop cancellation remain covered by the model/IO paths or the platform recipe below; they were not separately claimed as native gestures. Keyboard settings and CJK sources were not changed.

## Repeat the remaining platform checks

Use a temporary configuration and a saved copy of the bundled sample. Keep existing windows open. A drop currently inserts/replaces at the source selection, as documented in [writing workflows](writing-workflows.md); pointer-based drop placement is not introduced here.

1. Repeat the verified PNG workflow with JPEG/GIF/WebP from Finder. Verify one figure, durable local asset, Cmd+Z/Cmd+Shift+Z, and continued keyboard selection across adjacent blocks. Repeat a drop while a reverse selection is already active.
2. Drop an image into an unsaved document, cancel first Save, and verify selection, source and Undo. Drop a Markdown file over unsaved writing, cancel the document-change guard, and verify the same invariants.
3. With an already enabled Japanese input method, compose `nihongo`, inspect candidates, choose 日本語 and commit. Repeat Chinese `zhongwen` → 中文 and Korean two-set `gksrmf` → 한글. Test heading/body/table-cell targets, replacement of forward/reverse selections, cancellation followed by moving and recomposing, and one-step Undo/Redo.
4. Check candidate placement at wrapped lines and near the viewport bottom in both Live and Source modes. Confirm no duplicate insertion, stale replacement, lost focus or unexplained scroll jump. Leave input-source settings as found.

## Automated evidence

Run serially from the repository root:

```sh
mise run check
mise run release
DAYTRIP_PROFILE_BLOCKS=10000 DAYTRIP_PROFILE_SAMPLES=30 mise run profile
DAYTRIP_PROFILE_BLOCKS=50000 DAYTRIP_PROFILE_SAMPLES=30 mise run profile
```

The profiles exercise keyboard input, disclosure, selection/dragging, scrolling, IME, Undo/Redo and derived-state oracles through software rendering. They exclude OS delivery, candidate windows and GPU presentation. Other desktop work was active, so these are regression runs, not a controlled performance comparison. No speedup or input-to-photon claim is made.

Final validation passed: **156 ordinary tests**, formatting, Clippy with warnings denied for application targets, release build, and both interaction profiles. The initial sandboxed check could not bind the HTTP image test's localhost listener; the complete check passed with localhost access. Existing vendored Yrs warnings remain outside this change.

The [macOS bundle build and signature verification](validation/native-writing/bundle.txt) also passed. Replacing the executable uses the existing atomic bundle script; no existing window was restarted. The post-fix PNG UI session ran only after the user made the desktop available.

| Profile observation | 10k blocks | 50k blocks |
| --- | ---: | ---: |
| Cold App/source initialization, ms (one sample) | 155.99 | 715.53 |
| Middle typing, ms p50 / p95 | 2.076 / 3.268 | 5.891 / 7.895 |
| Disclosure, ms p50 / p95 | 3.118 / 3.641 | 16.046 / 17.076 |
| Half-document selection, ms p50 / p95 | 1.571 / 1.744 | 2.691 / 3.135 |
| Scrolling, ms p50 / p95 | 1.521 / 1.719 | 1.587 / 1.849 |
| IME commit, ms (one sample) | 3.002 | 7.208 |

The fixtures remain 910,450 / 4,574,450 source bytes with a 720×800 software viewport and 30 warm samples. Full phase breakdowns, tails and history interactions are in the raw logs: [check](validation/native-writing/check.txt), [release](validation/native-writing/release.txt), [10k](validation/native-writing/profile-10k.txt), [50k](validation/native-writing/profile-50k.txt), [environment and source hashes](validation/native-writing/environment.json). Existing document-storage growth and large-import limitations remain as recorded in [writing workflows](writing-workflows.md).
