# A complete writing document

Follow-up [macOS clipboard, drop, and input-method validation](native-writing-validation.md) records native observations, the canceled-composition target fix, automated coverage and remaining platform checks.

Koan's **The quiet hour · sample** sidebar action opens an independent local copy of a representative essay. It contains prose, headings, a linked reference, a photograph with alt text/caption/credit, an attributed quotation, a table and a checklist. The sample and its image are bundled and work offline. Each invocation uses a fresh directory under the application configuration's `samples/` folder; editing one copy never replaces another. Save As moves it into your own folder.

## Writing and details

Newsreader remains the writing face, with the existing heading scale and 720-pixel column. Blank Markdown lines now use 65% of a body row, reducing excess space between paragraphs and around captions. Measured full layout, virtual layout and virtual height estimates share the same metric. This changes local geometry deliberately; source positions, selection direction, wrap affinity and viewport anchoring keep their existing meanings.

- **Link:** select a phrase and use Cmd/Ctrl+K or ↗. The details show the destination; Apply creates or edits the Markdown and Unlink retains the label. A destination-only change preserves the label's existing inline formatting and Markdown title. Reference-style links normalize to inline links when edited. Pasting a web/email URL over selected text creates a link. Cross-paragraph selections are not converted to a single link.
- **Opening links:** a normal click edits; Cmd/Ctrl-click opens. HTTP/HTTPS and email use the OS handler. `#heading-slug` navigates locally. Relative `.md`, `.markdown` and `.txt` links open through Koan's unsaved-document guard. Unsupported schemes produce a message. The details dialog also has an Open action. Cross-file fragments and a link hover card remain future work.
- **Image:** use Image to add a URL or choose a file, paste one copied image file or raster clipboard image, or drop PNG/JPEG/GIF/WebP files. A drop inserts at the current source selection. Double-click an existing image, or put the caret in its caption/credit and use Image, to edit alt text, caption and credit. Images fit the column without upscaling, preserve aspect ratio and cap height at 520 pixels times zoom. GIFs retain their original bytes and show their first frame. Width/height overrides and cropping are not introduced.
- **Quotation:** ❝ creates or edits a quotation with author, source title and optional source destination. The quotation field accepts Markdown. The credit remains a line inside the blockquote.

Details dialogs keep stable, expected-text-checked source targets. Unrelated edits rebase them; an overlap leaves the dialog open with an explicit conflict and changes no source. Each Apply is one human transaction and one undo group. No presentation state is replicated. New forms escape label/caption/credit punctuation; ordinary source editing remains available for more elaborate Markdown.

Clipboard reads, decoding, asset copying and export run on blocking workers. Deferred paste/import targets keep insertions before text typed later at the same gap. A changed selected target collapses instead of deleting intervening content. If the user has continued writing or moved the selection, completion restores that current stable selection. Completion does not take focus from an open details/search field or cancel its transient composition. Targets from an earlier document session cannot apply. IME preedit remains transient; commit still enters the existing exactly-once composition transaction path. No native widget is replaced by these tools.

The portable figure convention is:

```md
![Description of what the photograph shows](<essay.assets/content-hash.jpg>)

*A caption with a little context.*

Photo: [Photographer](<https://source.example/photo>)
```

The quotation convention is:

```md
> Words worth carrying forward.
>
> — Author, [Book or essay](<https://source.example/essay>)
```

The controls recognize their standalone caption/`Photo:` convention without consuming following ordinary prose. Credits are editable links and text in the same live editor. They have no hidden database or required custom Markdown extension.

## Assets and saving

An unsaved document asks for its first save before importing an image, so every imported reference has a durable base folder. Canceling that save cancels the queued import. Native file chooser, copied image file/raster paste and drop share the same importer. Add images individually; multiple copied image files return an explicit message. Original supported image bytes are validated, hashed, and published without replacement into `<document stem>.assets/`; filenames are content hashes and references are URL encoded. Repeated imports reuse identical bytes. Clipboard RGBA becomes PNG. Existing files with different contents and preexisting symlink asset destinations fail explicitly. Files and directory entries are synchronized before Markdown publication. Unreferenced imported assets remain for undo; there is no automatic asset deletion. A separate session ledger retains local image references even when decoded images are evicted. It is bounded to 16 MiB of URI text and 65,536 entries; if exhausted, ordinary saving still works but moving to another folder requires saving/reopening to start a new undo session. No source edits or CRDT operations are dropped.

Limits are 16 MiB encoded per image, 8192 pixels per dimension, and 64 MiB decoded. The native clipboard API materializes its image before Koan can inspect its size; oversized clipboard data is rejected once returned. Network image loading retains its existing independent limits/cache. Imported assets are not replicated inside the text CRDT: an offline replica must carry the referenced asset folders too.

Save As copies current and session-history relative assets into the destination folder **without changing source paths**, before using the existing journaled Markdown/CRDT save. Existing identical assets are reused; conflicting bytes are never overwritten. This preserves undo references as well as source bytes. Relative paths escaping the document folder (`..`), symlink asset destinations and a copy exceeding 128 MiB stop Save As explicitly. Absolute/file-URL references remain absolute. Missing relative assets stop relocation. A first save establishes the base for manually written relative references without attempting relocation. Previously broken historical references are skipped; existing images in Undo history are copied even when absent from the current source. A failed relocation may leave harmless unused assets, but does not publish new Markdown or change the active document path. Ordinary saves retain all existing external-file, checkpoint, permissions and symlink guards; edits made during saving remain dirty.

## Recovery and export

Unsaved writing receives a checksummed **single-file** compatible CRDT checkpoint after two idle seconds, or after 30 seconds of continuous writing (one-second scheduler, active only while a dirty generation needs a checkpoint). It includes the saved source/checkpoint baseline, path, display name and directional source selection. Serialization/IO run in the background; full CRDT snapshot extraction is an explicit recovery operation, never a per-keystroke extraction. Each document session gets a fresh random recovery filename. Recovery does not reuse a persisted writer identity.

Startup points out available recovery copies. **Recovery copies** in the sidebar lists them by name and recency; Open copy uses the ordinary Save/Discard/Cancel guard. Restoring creates a fresh writer from compatible CRDT state and keeps the original saved-file baseline, so a subsequent Save detects external changes; Save As keeps both. No disk document is automatically overwritten. A partial temporary file cannot replace a valid recovery copy. Corrupt, oversized or unsupported copies return controlled errors and remain on disk.

A successful clean save or explicit discard retires this session's recovery copy. A late in-flight write is removed only after it finishes, preventing resurrection. Earlier/crashed/previously restored copies remain until explicitly removed from the recovery folder (Show folder opens it). Other windows' recovery copies are never automatically deleted. Snapshots retain full compatible CRDT state; local in-memory undo stacks are not persisted. Up to 30 seconds plus scheduler/write time of the newest work may be absent after a crash; pauses normally checkpoint after two seconds. Disk-full/permission errors are surfaced; recovery is not a substitute for saving. Recovery files are bounded to 112 MiB and inherit private temporary-file permissions; the recovery directory is private on Unix.

**Export HTML** captures a source revision and embeds local images as data URLs, preserving captions, credits, alt text, tables and readable typography in one file. It does not require copying an asset directory to view the export. Missing/invalid local images stop export rather than silently breaking the result. Embedded image payloads are capped at 128 MiB. HTTP/HTTPS images remain remote; export does not fetch them. Raw HTML is escaped, unsupported link schemes are disabled, and a restrictive CSP prevents script execution. Exports use the existing atomic writer. PDF/office export is deferred.

## Validation

A full-widget focus regression verifies that opening each details dialog settles redraws at 760×520 and 1180×820. Focus updates must continue through modal/loading guards, even while source edits are blocked. The real App regression opens the representative source, changes its link while a compatible replica edits elsewhere, tests undo/redo, edits its caption and quotation credit, converges, saves/reopens with a fresh writer, and exports with an embedded local image. Further tests cover expected-text conflicts, mixed CRLF/Unicode neighbors, exact undo, delayed clipboard target deletion, continued typing before paste completion, IME commit history, immutable assets, Save As, symlinks/traversal, corrupt/interrupted recovery, and late recovery completion. Existing CRDT convergence, selection, disclosure, native-buffer, wrap/viewport and IME event tests remain required.

Release/profile commands and measurements are recorded below. Automated software rendering does not validate OS clipboard codecs, native drag/drop delivery, or a real IME candidate window on every platform. Linux/Windows native clipboard and CJK candidate interaction need manual platform validation.

## Sample image provenance

`assets/writing/coast.png` was generated with OpenAI's image generation tool. It is explicitly credited as an illustrative generated image in the sample; no real photographer or location is asserted. The checked-in PNG is the application asset; building and testing require no external image library or private generation directory.

Exact generation prompt:

> Create one natural editorial landscape photograph for a quiet, beautiful writing application's sample essay called The quiet hour. Wide 3:2 landscape composition: a serene rocky Pacific coastline at dawn, pale warm peach horizon, soft sea mist, muted sage coastal grasses in the foreground, calm silver blue water and a single modest headland. Authentic film photography, delicate grain, refined restrained colors, generous breathing room, subtle morning light, no people, no buildings, no text, no lettering, no logo, no watermark. This is a sample illustrative photograph, not a real credited photograph.

## Reproducible performance

Measured 2026-10-23, Apple M2 Max / 32 GiB, macOS 26.7.1, mise Rust 1.95.0, Iced 0.14, release / thin LTO / one codegen unit. The clean baseline is **34d6cc3**, which already contains the completed bounded offline-import optimization. Its preserved executable was used for fresh interaction profiles before editing and for a paired import/memory follow-up after final builds. [Environment](performance/writing-workflows/environment.txt), [source hashes](performance/writing-workflows/source-sha256.txt), and [executable hashes](performance/writing-workflows/executables-sha256.txt) identify the runs.

All final builds and measurements ran serially, with the isolated native test process closed. Existing 910,450 / 4,574,450-byte fixtures, 720×800 viewport and 30 warm samples are unchanged. These integrated editor profiles include CRDT, source/projection/native adapters, layout, draw and software rasterization. They exclude window chrome, dialog/asset IO, GPU presentation and OS event delivery. The sample photograph is not decoded in this fixture. They are not a claim about end-to-end OS input latency. Full source/projection oracles remain outside timed paths.

Times below are total **p50 / p95 milliseconds**, before → after. Cold initialization and IME/history each have one sample; do not interpret them as distributions or speedup evidence. Raw logs retain all phases and maxima. The denser paragraph spacing deliberately raises initial resident fragments/rows from **44/52 to 52/60**; this is a visual change with more text on screen, rather than a performance optimization.

### 10k blocks

| Interaction | Before | After |
| --- | ---: | ---: |
| Cold source/App initialization, ms | 167.0 | 148.2 |
| Typing at start | 1.895 / 2.851 | 2.120 / 2.714 |
| Typing in middle | 1.728 / 2.205 | 2.176 / 2.809 |
| Typing at end | 1.527 / 1.912 | 1.805 / 2.212 |
| Disclosure | 2.913 / 3.399 | 3.084 / 3.622 |
| Cross-block selection | 1.282 / 1.332 | 1.586 / 1.802 |
| Scroll | 1.312 / 1.732 | 1.571 / 1.916 |
| IME commit (one sample) | 7.812 / 7.812 | 3.420 / 3.420 |
| Undo IME (one sample) | 2.020 / 2.020 | 2.823 / 2.823 |
| Redo IME (one sample) | 1.893 / 1.893 | 2.087 / 2.087 |

[Before](performance/writing-workflows/before-10k.txt), [after](performance/writing-workflows/after-10k.txt).

### 50k blocks

| Interaction | Before | After |
| --- | ---: | ---: |
| Cold source/App initialization, ms | 838.5 | 700.5 |
| Typing at start | 6.207 / 8.146 | 6.754 / 10.248 |
| Typing in middle | 4.933 / 6.840 | 5.784 / 7.670 |
| Typing at end | 5.358 / 6.649 | 5.065 / 5.757 |
| Disclosure | 14.809 / 18.672 | 14.858 / 16.006 |
| Cross-block selection | 2.640 / 3.249 | 2.652 / 2.888 |
| Scroll | 1.520 / 1.831 | 1.551 / 1.826 |
| IME commit (one sample) | 6.640 / 6.640 | 8.102 / 8.102 |
| Undo IME (one sample) | 5.209 / 5.209 | 4.813 / 4.813 |
| Redo IME (one sample) | 5.857 / 5.857 | 5.037 / 5.037 |

[Before](performance/writing-workflows/before-50k.txt), [after](performance/writing-workflows/after-50k.txt).

Several 10k medians increase by a fraction of a millisecond; rasterization does more work with the denser layout. No speedup is claimed for these workflow changes. At 50k, typing medians span 5.1–6.8 ms, scroll is 1.55 ms, and disclosure is 14.86 ms. Some tails remain outside a frame budget: the first end-of-document typing/capacity-growth event is **113.5 → 111.7 ms**. This remains a concrete limit of contiguous source/projection/native storage. Single IME/undo samples vary and do not establish a regression or improvement.

### Offline imports, retained state and memory

The bounded import work is retained: 32 certified independent windows at most 64 KiB each; larger/uncertain batches still use the full scanner without changing or dropping CRDT operations. Paired 30-sample, eight-edit imports measured:

| Blocks | Before p50 / p95, ms | After p50 / p95, ms |
| --- | ---: | ---: |
| 10,000 | 9.090 / 11.015 | 9.340 / 11.508 |
| 50,000 | 38.238 / 40.781 | 38.409 / 43.059 |

The paired baseline/final processes peaked at **563.8 / 569.1 MiB RSS**, with macOS peak memory footprints **512.8 / 517.7 MiB**. Current RSS after 50k churn was **487.5 / 485.8 MiB**; allocation retention varies and is not a live-object measurement. Earlier diagnostic final runs showed higher memory than the previous report, so a current paired baseline was run instead of attributing that difference to the feature. Both binaries now show similar process high-water marks. [Paired baseline](performance/writing-workflows/before-collaboration.txt), [final](performance/writing-workflows/collaboration.txt), and [earlier diagnostics](performance/writing-workflows/earlier/README.md) are retained. No decoded-photo/cache allowance is included in those figures. Large text documents still have substantial transient memory costs.

After 200 insertion/undo pairs, complete checkpoint sizes were identical before/after: **916,791 → 921,029 bytes at 10k**, **4,580,919 → 4,585,159 bytes at 50k**. The 2,000-transaction component churn/reopen benchmark also passed; [benchmark output](performance/writing-workflows/bench.txt) separates CRDT and source costs. Retention/undo policy has not been compacted away to obtain these numbers.

[Extended imports](performance/writing-workflows/import-shapes.txt) run ten batches per shape and size, checking source/projection/native equivalence. The certified 32-window and structural paths pass 10/10; 128 edits deliberately fall back 10/10. A first large fallback remains a conspicuous offline-import stall (about 0.85 seconds in the final 50k run); this is not a real-time multiplayer guarantee. These extended cases are validation, not a matched speedup comparison.

Reproduce from the repository root:

```sh
mise run check
mise run release
RUST_TEST_THREADS=1 mise run bench
KOAN_PROFILE_BLOCKS=10000 KOAN_PROFILE_SAMPLES=30 mise run profile
KOAN_PROFILE_BLOCKS=50000 KOAN_PROFILE_SAMPLES=30 mise run profile
KOAN_PROFILE_SAMPLES=30 mise exec -- cargo test --locked --release large_document_collaboration -- --ignored --nocapture --test-threads=1
KOAN_PROFILE_SAMPLES=10 mise exec -- cargo test --locked --release large_offline_import_shapes -- --ignored --nocapture --test-threads=1
```

Final validation: **152 ordinary tests**, **7 serial release benchmarks**, both interaction profiles and both import/churn profiles passed. [Check log](performance/writing-workflows/check.txt), [release build](performance/writing-workflows/release.txt), and [signed bundle validation](performance/writing-workflows/bundle.txt) are retained. macOS native smoke testing verified the sample's visual layout, double-click image details, caption editing, Cmd+K details and saving. It exposed the modal focus loop now covered by the full-widget regression. Desktop activity changed during testing, so further UI input was stopped and only the isolated saved test process was closed. Native clipboard/file-list paste, drag/drop delivery, real CJK candidate windows, export chooser completion, and small-window visual behavior still need platform QA; automated tests cover their model/IO paths.
