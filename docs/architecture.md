# Architecture

`App` owns application state and follows Iced's `update → state → view` model. File operations run in background tasks and return typed messages. The active resolved theme is stored in `App` and shared by styles, text highlights, and canvas icons.

| Module | Responsibility |
| --- | --- |
| `app.rs` | Commands, continuous native editing buffer, file guards, UI composition |
| `projection.rs` | Markdown-to-display projection, source mapping, caret affinity, edit translation |
| `projection_update.rs` | Conservative incremental projection of word edits |
| `projection_disclosure.rs` | Line-scoped disclosure highlighting with shared unchanged styles |
| `performance.rs` | Ignored large-document event/layout/software-render profiles |
| `selection.rs` | UTF-8 byte selections and line/column conversion |
| `rich_document.rs` | Heading, table, and image metadata collected during projection parsing |
| `rich_layout.rs` | Cached shaped paragraphs and shared geometry for drawing, caret, hit testing, and navigation |
| `rich_editor.rs` | One live widget: focus, clipboard, mouse/keyboard selection, scrolling, IME |
| `table_edit.rs` | Cell-aware source edits and rectangular TSV paste |
| `image_store.rs` | Asynchronous bounded image IO, decoding, and document-scoped cache |
| `document.rs` | Authoritative Markdown, cached blocks, reversible patches and selection history |
| `theme.rs` | Strict theme schema, validation, semantic tokens, resolved Iced theme |
| `styles.rs` | Central widget styles and typography |
| `markdown_view.rs` | Full Markdown layout in optional Read view |
| `viewport.rs` | Measured block navigation in Read view |
| `icon.rs` | Canvas icons consuming the resolved tokens |
| `storage.rs` | Bounded UTF-8 reads, atomic writes, conflict checks, preferences, HTML export |

## Continuous editing

The live editor is one Iced `text_editor::Content` covering the whole document. `RichEditor` is a custom Iced widget over that buffer, rather than a collection of per-block editors. Native actions handle text mutations and logical motions. Geometry-dependent movement, selection drawing, pointer hit testing, scrolling, and input-method positioning share `RichLayout`'s measured paragraphs. There is one focus and selection across the page.

The authoritative Markdown remains in `Document`. A `Projection` contains the visible text, compact source/display pieces, paired delimiter ranges, immutable highlight runs, and a shared source snapshot. While focused, the heading containing the selection head reveals its prefix (and optional closing hashes or Setext underline). Only the innermost inline span containing that head reveals its own delimiters, including a link's destination. Other spans on the same line stay concealed. A span's closing endpoint belongs to the surrounding prose, so finishing a span conceals it immediately. Revealed syntax is muted, smaller monospace text; content keeps its semantic styles and heading size. Losing focus conceals syntax without changing source selection or history.

Mappings work in UTF-8 byte offsets, as does Iced/cosmic-text. Both native editing and rich shaping use the same visible projection. Concealed syntax takes no space, including long link destinations. Lists/tasks/quotes contribute their display glyphs directly to flow; table separators and image objects retain their structural replacements. Structural source revisions are parsed once; safe word edits rebase the existing parse. A disclosure change reuses that parse and source snapshot, rebuilding only the projected text and mapped metadata. Context is represented by exact marker ranges, so disclosing a heading does not expose unrelated inline spans or nested markers.

At an offset beside hidden syntax, two source positions may correspond to one displayed caret. The source selection retains that affinity. Pointer hits, vertical navigation, drag anchors, and selection rectangles map through the same source/display pieces. Hidden syntax adds no arrow stops. Absolute document motions target source endpoints, including concealed trailing delimiters. Wrap affinity records whether an ambiguous caret belongs upstream or downstream and is resolved against the new layout after disclosure. Preferred horizontal position survives these layout changes.

Disclosure can reflow the affected paragraph or heading and move later blocks vertically. Before relayout, the widget anchors the visible source caret's screen Y; if the caret is offscreen, it anchors the first visible source row instead. After relayout it adjusts the scroll offset by that anchor's displacement, clamping to the document extent. Focus changes do not scroll an offscreen caret into view. Ordinary navigation still brings its target into view. Horizontal caret position may change with the local text flow.

An edit is translated into a source splice. Selection replacements honor their full original range even when the inserted text shares a prefix or suffix. Visible delimiters edit literally. Edits crossing concealed delimiter boundaries repair the surviving pairs; deleting the last character of a concealed span removes empty wrappers. CommonMark's flanking rules cannot represent every rich-text join, especially around punctuation/emoji. When a repair would expose invalid delimiters, the affected inline spans are normalized to escaped plain text, preserving their displayed characters. This may clear formatting on that affected span, never the rest of the document.

The existing native buffer receives a minimal text splice when source edits or syntax disclosure change the display. Navigation borrows the previous projection as its text snapshot; a native edit assembles its resulting buffer only once. Opening a different document initializes a native buffer directly, avoiding the costly large paste path. The widget emits source selections and stores drag anchors in source coordinates, so revealing markers between pointer events cannot shift the selected text. Rich widget state survives edits, including preferred horizontal caret position and preedit state. Iced's input-method API positions its composition overlay at the measured caret; only committed text enters the document model. Real CJK IME testing across platforms remains necessary.

## Selection, formatting, and history

A source `Selection` has separate anchor/head offsets, preserving reverse selection. Native movements update it through the projection. Restoring a caret explicitly clears any native selection because Iced 0.14's `Content::move_to` retains an existing anchor when given `selection: None`. Switching source/live mode maps both endpoints without changing the document. Source-only search matches, such as a hidden URL, open source mode so the selection remains inspectable.

Bold, italic, and inline-code formatting operate separately on selected lines and preserve heading/list prefixes. Commands use a concealed projection so active syntax does not change their selection semantics. Exact selected marks can be toggled off. Enter continues ordered/unordered/task lists and blockquotes; Enter in an empty item exits it. Disclosed heading prefixes support ordinary caret movement, selection, and literal deletion.

Undo patches store source splices plus before/after source selections. Consecutive edits within one inserted region coalesce for 650 ms. Movements and explicit commands end the group. New edits after undo clear redo. The history budget is 16 MiB, retaining the last operation even if it alone exceeds the budget.

## Parsing and performance

Pulldown-cmark re-establishes top-level document ranges after source edits. Unchanged blocks reuse their rendered Markdown caches; only changed ranges are discarded. Document storage is still contiguous and top-level structure is still reparsed per edit.

The projection has a conservative incremental path for ASCII word characters and interior spaces strictly within an existing parsed text run. It rejects delimiter edges, links, image syntax, raw HTML, definitions, code fences, newlines, and other structural changes. Accepted edits rebase source/display pieces and semantic ranges, share unchanged line highlight arrays, and update only the affected line's styles. A full parse handles all rejected cases. Differential tests compare accepted edits with full parsing, including every source/display mapping.

Disclosure rebuilds the compact text/map but repaints only affected physical lines. Unchanged line styles are shared through `Arc` slices. Setext underlines and multiline marker ranges use the general path. Sorted change regions describe the predecessor projection by highlight identity, so skipped or unrelated revisions cannot accidentally patch stale geometry.

Rich layout caches both shaped paragraphs and local glyph/grapheme caret stops. For ordinary paragraphs with unchanged metrics, it reshapes only changed paragraphs, rebases exact source offsets and vertical geometry below them, and rebuilds the visibility indexes. This also supports disclosure changes in several paragraphs. Tables, image flow, multiline structural edits, theme/width/zoom changes, and unrelated revisions use full layout. Intermediate edited paragraph shapes are not accumulated in the cache; current rows and fragments own the shared geometry they need. Differential tests compare incremental layouts with fresh layouts over repeated reflow.

Caret lookup uses source-ordered row ranges. Vertical interval indexes account for the different source and Y ordering inside wrapping table cells. Drawing submits only visible fragments, decorations, and selection rectangles, so selecting half a document does not submit thousands of offscreen quads. Table lookup converts the selection once and binary-searches table/row ranges. Drawing never parses Markdown.

Initial shaping and retained geometry still cover the entire document. This is incremental layout and viewport-limited painting, not viewport virtualization. Structural edits, table relayout, long paragraphs, outline construction, native buffer initialization, and retained memory remain scaling limits; the measurements and reproduction commands are in [the performance report](performance.md).

## Rich blocks

Table cells retain both source and display ranges. Source-only pipes, padding, and alignment rows project to tabs and row breaks. Cells wrap independently, with shared row heights and column alignment. Vertical movement skips unavailable lines in an adjacent taller cell; horizontal movement and selections remain document-wide. Cell-aware input preserves separators, escapes pipes, supports rectangular TSV paste, and adds rows. Inline formatting works independently per selected cell.

Images project to an atomic object replacement character. Their complete Markdown remains behind that object. Selection, deletion, clipboard transfer, source switching, and undo map through the same source pieces as text. Images occupy block flow, including images written inline and images inside table cells. Local/file/HTTP/HTTPS loading and decoding run outside the UI thread, with four concurrent loads, a 20-second HTTP timeout, redirect limits, a 16 MiB encoded cap, decoder allocation/dimension limits, and a 128 MiB document image cache. Renderer handles contain decoded RGBA pixels. Failed images retain their object and show a message. GIFs show the first frame; SVG is unsupported.

## Files

File reads/writes run in Tokio blocking tasks. Reads are capped at 16 MiB even if a file grows. Opening locks document edits until completion. Saving captures a snapshot; edits made while it saves remain dirty.

Existing-file saves compare disk contents with the loaded/saved baseline, write a temporary file beside the destination, preserve permissions, sync, atomically persist, and sync the directory on Unix. Symlinks resolve to their target. Conflict detection is best effort: another process can write between comparison and replacement. Recovery and interprocess locking remain future work.

HTML export escapes raw Markdown HTML and includes a restrictive content security policy. Live images resolve relative to the saved document; remote URLs are fetched automatically. The optional older Read renderer still uses local image handles and remote placeholders. Preferences contain appearance and recent paths, and writes are serialized.
