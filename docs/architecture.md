# Architecture

Still is one Cargo package with a thin `main.rs` calling `still::run()`. The library root declares responsibility-based modules and exports only that application entry point. Implementation modules stay crate-private; a separate crate or public model API needs a concrete consumer or build-time reason.

`App` owns application state and follows Iced's `update → state → view` model. File operations run in background tasks and return typed messages. The active resolved theme is stored in `App` and shared by styles, text highlights, and canvas icons.

| Module | Responsibility |
| --- | --- |
| `app/startup.rs` | Native startup, fonts, Iced event-loop configuration |
| `app/application.rs` | Application state, messages, commands, native buffer, file guards, views |
| `app/icon.rs` | Canvas icons consuming the resolved tokens |
| `app/performance.rs` | Private ignored large-document event/layout/software-render profiles |
| `document/source.rs` | CRDT transaction boundary, incremental source cache and cached blocks |
| `document/collaboration.rs` | Yrs ownership, coordinate bridge, stable intent, scoped undo and proposals |
| `document/collaboration_wire.rs` | Bounded admission of plain-text Yjs update V1 |
| `document/block_parser.rs` | Top-level ranges, independent block candidates, conservative reparse windows |
| `document/selection.rs` | UTF-8 byte selections and line/column conversion |
| `document/projection.rs` | Markdown-to-display projection, source mapping, caret affinity, edit translation |
| `document/projection_update.rs` | Conservative incremental projection of word edits |
| `document/projection_blocks.rs` | Local block projection, retained metadata, styles and mapping splices |
| `document/projection_disclosure.rs` | Line-scoped disclosure highlighting with shared unchanged styles |
| `document/rich_document.rs` | Heading, table, and image metadata collected during projection parsing |
| `document/table_edit.rs` | Cell-aware source edits and rectangular TSV paste |
| `editor/rich_layout.rs` | Cached shaped paragraphs and shared geometry for drawing, caret, hit testing, navigation |
| `editor/virtual_layout.rs` | Lightweight height index, viewport materialization, geometry eviction, source jumps |
| `editor/native_buffer.rs` | Standard Iced buffer ownership, deferred loading, physical line index and reusable text snapshot |
| `editor/rich_editor.rs` | One live widget: focus, clipboard, mouse/keyboard selection, scrolling, IME |
| `editor/source_editor.rs` | Native Source widget delegation with stable IME commit targets |
| `editor/markdown_view.rs` | Full Markdown layout in optional Read view |
| `editor/viewport.rs` | Measured block navigation in Read view |
| `theme/definition.rs` | Strict theme schema, validation, semantic tokens, resolved Iced theme |
| `theme/styles.rs` | Central widget styles and typography |
| `io/image_store.rs` | Asynchronous bounded image IO, decoding, and document-scoped cache |
| `io/storage.rs` | Bounded reads, atomic writes, preferences and HTML export |
| `io/collaboration_store.rs` | Checkpoints, recovery journal, file locks and offline interchange |

Tests live beside private implementations. Shared Markdown inputs belong in `tests/fixtures/`; integration tests belong in `tests/` when they exercise a deliberate public API. Profiling remains a private application test so it can exercise the real event path without exporting editor internals.

## Continuous editing

The live editor is one Iced `text_editor::Content` covering the whole document. `RichEditor` is a custom Iced widget over that buffer, rather than a collection of per-block editors. Native actions handle text mutations and logical motions. Geometry-dependent movement, selection drawing, pointer hit testing, scrolling, and input-method positioning share `RichLayout`'s measured paragraphs. There is one focus and selection across the page.

The authoritative Markdown remains in `Document`. A `Projection` contains the visible text, compact source/display pieces, paired delimiter ranges, immutable highlight runs, a retained source String, and a separate source revision identity. While focused, the heading containing the selection head reveals its prefix (and optional closing hashes or Setext underline). Only the innermost inline span containing that head reveals its own delimiters, including a link's destination. Other spans on the same line stay concealed. A span's closing endpoint belongs to the surrounding prose, so finishing a span conceals it immediately. Revealed syntax is muted, smaller monospace text; content keeps its semantic styles and heading size. Losing focus conceals syntax without changing source selection or history.

Mappings work in UTF-8 byte offsets, as does Iced/cosmic-text. Both native editing and rich shaping use the same visible projection. Concealed syntax takes no space, including long link destinations. Lists/tasks/quotes contribute their display glyphs directly to flow; table separators and image objects retain their structural replacements. Structural source revisions are parsed once; safe word edits rebase the existing parse. Source updates splice retained text and parsed metadata in place. A disclosure change retains the parse, source buffer and revision identity while rebuilding projected text and mapped metadata. Context is represented by exact marker ranges, so disclosing a heading does not expose unrelated inline spans or nested markers.

At an offset beside hidden syntax, two source positions may correspond to one displayed caret. The source selection retains that affinity. Pointer hits, vertical navigation, drag anchors, and selection rectangles map through the same source/display pieces. Hidden syntax adds no arrow stops. Absolute document motions target source endpoints, including concealed trailing delimiters. Wrap affinity records whether an ambiguous caret belongs upstream or downstream and is resolved against the new layout after disclosure. Preferred horizontal position survives these layout changes.

Disclosure can reflow the affected paragraph or heading and move later blocks vertically. Before relayout, the widget anchors the visible source caret's screen Y; if the caret is offscreen, it anchors the first visible source row instead. After relayout it adjusts the scroll offset by that anchor's displacement, clamping to the document extent. Focus changes do not scroll an offscreen caret into view. Ordinary navigation still brings its target into view. Horizontal caret position may change with the local text flow.

An edit is translated into a source splice. Selection replacements honor their full original range even when the inserted text shares a prefix or suffix. Visible delimiters edit literally. Edits crossing concealed delimiter boundaries repair the surviving pairs; deleting the last character of a concealed span removes empty wrappers. CommonMark's flanking rules cannot represent every rich-text join, especially around punctuation/emoji. When a repair would expose invalid delimiters, the affected inline spans are normalized to escaped plain text, preserving their displayed characters. This may clear formatting on that affected span, never the rest of the document.

The existing native buffer receives minimal text splices when source edits or syntax disclosure change the display. Proven predecessor regions are applied from the end, so moving disclosure between distant headings does not replace the unchanged document between them. The native buffer wrapper owns the standard `Content`, a reusable text snapshot, and an index of its physical line starts. Widgets receive immutable native content; every mutation goes through the wrapper. After a native edit, it reads the span between the selected endpoints and one neighboring line on either side, adjusting the window by the native line-count change. This covers insertion, paste/IME commit, Enter, deletion and indentation without reproducing Unicode editing rules. It splices the snapshot and rebases suffix offsets. Navigation reads no native text, and cursor conversion uses the same physical index instead of scanning the document. The index follows native lines even when concatenated mixed endings could be interpreted differently. Synchronization splices expand endpoints inside CRLF/LFCR to complete native endings, preserving the unchanged bytes in the replacement.

Document loading populates an empty native buffer with batches of physical lines, avoiding both Iced 0.14's eager whole-document shaping and cosmic-text's quadratic insertion of one large paste. It keeps the standard `Content` and native actions; text remains document-wide.

The widget emits source selections and stores drag anchors in source coordinates, so revealing markers between pointer events cannot shift the selected text. Rich widget state survives edits, including preferred horizontal caret position and preedit state. Wrap affinity is stored as upstream/downstream intent, independently of evicted geometry's Y coordinates. Iced's input-method API positions its composition overlay at the measured caret; only committed text enters the document model. Real CJK IME testing across platforms remains necessary.

## Selection, formatting, and history

A source `Selection` has separate anchor/head offsets, preserving reverse selection. Native movements update it through the projection. Restoring a caret explicitly clears any native selection because Iced 0.14's `Content::move_to` retains an existing anchor when given `selection: None`. Switching source/live mode maps both endpoints without changing the document. Source-only search matches, such as a hidden URL, open source mode so the selection remains inspectable.

Bold, italic, and inline-code formatting operate separately on selected lines and preserve heading/list prefixes. Commands use a concealed projection so active syntax does not change their selection semantics. Exact selected marks can be toggled off. Enter continues ordered/unordered/task lists and blockquotes; Enter in an empty item exits it. Disclosed heading prefixes support ordinary caret movement, selection, and literal deletion.

Yrs owns the authoritative text and scoped human/agent undo. Source ranges are UTF-8 bytes translated through an incremental UTF-16 index. Local source edits, rich repair, accepted proposals and imported operations enter one document transaction path. Source/projection/native/layout caches remain derived; local generations are distinct from document identity and causal versions. Stable positions preserve intent through intervening edits. See [collaboration](collaboration.md) for transaction, undo, IME and proposal contracts.

## Parsing and performance

Pulldown-cmark establishes top-level document ranges on load. After an edit inside an eligible block, the source model reparses that block's isolated window (at most 64 KiB), splices replacement blocks and adjusts suffix offsets and statistics. Candidates are unindented paragraphs, headings, tables and rules with unchanged blank-line barriers and safe neighbors. Containers, HTML, fences, definitions, footnotes, uncertain boundaries and large windows use the full parser. Documents containing reference definitions or footnotes retain the full source-parsing path. Overlapping parser offsets are kept as one non-incremental range. Undo/redo applies the same change path once; it does not reparse a second time on commit.

The projection retains its fast path for ASCII word characters and interior spaces strictly within an existing parsed text run. It owns its source/display Strings and parsed vectors. A small `Arc<()>` identifies the source revision for layout, independently of text allocation; every source edit replaces that identity, while disclosure retains it. Word edits preflight the active context before mutating retained metadata. Equal-length word replacements retain mapping/line-start snapshots and skip offset rebasing; other word edits adjust the relevant metadata suffix without deep-cloning image/table payloads. Other eligible edits use the source model's predecessor revision and certified window, validate the unchanged prefix/suffix, and parse only that fragment. A complete splice plan validates metadata containment and ordering before mutation. Parsed decorations, inline pairs, rich structures and safe text runs outside it retain their allocations with suffix offsets rebased in place. Displayed rich metadata is patched separately from source metadata, preserving each coordinate system. Line starts are spliced from the local result. Mapping construction reserves one successor buffer; old layout retains its immutable mapping, highlight revision and line index. Identity mapping pieces can be split at window boundaries; replacements may not cross them. Unaffected line highlight arrays stay shared. Disclosure changes outside the edited window and global dependencies use the general path. Plain paragraph changes omit an unchanged terminating newline from layout invalidation; structural windows remain broad so heading kinds, tables and image flow are rediscovered. Differential tests compare both paths with full parsing, every source/display mapping and fresh rich geometry.

Disclosure rebuilds the compact text/map but repaints only affected physical lines. Unchanged line styles are shared through `Arc` slices. Setext underlines and multiline marker ranges use the general path. Sorted change regions describe the predecessor projection by highlight identity, so skipped or unrelated revisions cannot accidentally patch stale geometry. Every accepted source update gets a fresh highlight revision, including equal-length edits that share unchanged mapping/index snapshots.

Rich layout caches both shaped paragraphs and local glyph/grapheme caret stops. Its exact full-document path, used for small documents and differential tests, can patch ordinary paragraphs with unchanged metrics, rebase geometry below them, and rebuild visibility indexes. Tables, image flow, multiline structural edits, theme/width/zoom changes, and unrelated revisions rebuild that exact layout. The virtual path described below builds only resident units and shares unchanged cached paragraphs within that window. Intermediate edited shapes are not accumulated indefinitely. Differential tests compare incremental and virtual geometry with fresh layouts over repeated reflow.

Caret lookup uses source-ordered row ranges. Vertical interval indexes account for the different source and Y ordering inside wrapping table cells. Drawing submits only visible fragments, decorations, and selection rectangles, so selecting half a document does not submit thousands of offscreen quads. Table lookup converts the selection once and binary-searches table/row ranges. Drawing never parses Markdown.

For documents above 256 physical layout units, `VirtualLayout` keeps only a viewport and one viewport of overscan on either side, plus adjacent units for navigation. A unit is a physical paragraph or a table row, so a large table does not require shaping the whole table. Smaller documents retain exact full layout. Each materialized unit uses the same `RichLayout` builder and paragraph cache as full layout; evicted paragraphs, glyph stops, fragments, and decorations are released. Only scalar height hints survive eviction.

The document-wide unit index stores offsets and estimated or measured heights. A Fenwick tree uses double-precision sums for height updates and position queries. Local changes with a proven projection predecessor rebase unit offsets and invalidate affected geometry; multiline or unrelated changes rebuild the lightweight index. Height fingerprints are estimates only. They never authorize reuse of shaped text, which still requires the exact paragraph cache key.

Pointer hits, visual motion, caret placement, and selection rectangles use measured resident geometry. Distant source navigation materializes the destination directly. A selection's remote anchor does not require shaping intervening text. Wheel scrolling, scrollbar movement, and drag autoscroll prepare the new window before drawing or hit testing. As estimates are refined, the first visible source row keeps its screen position. Disclosure, resizing, and theme/image metric changes preserve a visible source caret or viewport row; caret wrap affinity and selection direction remain source based. The scrollbar's estimated extent can refine while exploring previously unvisited text.

Context-sensitive fallback parsing, contiguous source/projection/native snapshot storage, immutable mapping/index copies, metadata scans and suffix rebasing, outline construction, and a single exceptionally long paragraph or table row remain scaling limits. The native mirror adds one retained String and physical line index; changing text length still shifts its suffix and rebases index offsets. Large selections and exceptionally long physical lines can still require large native reads, and capacity growth can copy the retained snapshot. Native shapes can accumulate on visited lines, and the stock Source widget can still shape extensively when changing metrics. The original measurements are in [the baseline report](performance.md), followed by [viewport virtualization](virtualization.md), [deferred native initialization](native-buffer.md), [incremental block parsing](incremental-parsing.md), [native snapshots](native-snapshots.md), and [source/projection updates](source-projection-updates.md).

## Rich blocks

Table cells retain both source and display ranges. Source-only pipes, padding, and alignment rows project to tabs and row breaks. Cells wrap independently, with shared row heights and column alignment. Vertical movement skips unavailable lines in an adjacent taller cell; horizontal movement and selections remain document-wide. Cell-aware input preserves separators, escapes pipes, supports rectangular TSV paste, and adds rows. Inline formatting works independently per selected cell.

Images project to an atomic object replacement character. Their complete Markdown remains behind that object. Selection, deletion, clipboard transfer, source switching, and undo map through the same source pieces as text. Images occupy block flow, including images written inline and images inside table cells. Local/file/HTTP/HTTPS loading and decoding run outside the UI thread, with four concurrent loads, a 20-second HTTP timeout, redirect limits, a 16 MiB encoded cap, decoder allocation/dimension limits, and a 128 MiB document image cache. Renderer handles contain decoded RGBA pixels. Failed images retain their object and show a message. GIFs show the first frame; SVG is unsupported.

## Files

File reads/writes run in Tokio blocking tasks. Reads are capped at 16 MiB even if a file grows. Opening locks document edits until completion. Saving captures a snapshot; edits made while it saves remain dirty.

Collaborative saves compare both Markdown and checkpoint baselines under an advisory interprocess lock. A synced journal makes publication of Markdown and CRDT metadata recoverable across both renames. Permissions, physical symlink targets and atomic writes retain their existing behavior. External Markdown/metadata mismatches are explicit conflicts. Full encoded state retains pending updates and uses a fresh active replica identity on every reopen. See [persistence and recovery](collaboration.md#persistence-and-recovery).

HTML export escapes raw Markdown HTML and includes a restrictive content security policy. Live images resolve relative to the saved document; remote URLs are fetched automatically. The optional older Read renderer still uses local image handles and remote placeholders. Preferences contain appearance and recent paths, and writes are serialized.

The current collaboration measurements follow in [CRDT performance](crdt-performance.md).
