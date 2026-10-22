# Iced evaluation

This is a technology evaluation, not a certification of production readiness.

## Verified locally

Rust 1.95.0, Iced 0.14.0, macOS on Apple silicon. Formatting, strict Clippy, and the regression suite run through `mise run check`.

Continuous-editing coverage includes contextual heading and inline-span disclosure, all six heading levels, literal marker deletion/undo, Setext underline editing, focus/caret restoration without history changes, same-context projection reuse, nested styles, Unicode source/display offsets, cross-block selections/replacements, reverse selections, list continuation, and committed Unicode input. An exhaustive range test replaces every visible selection in representative concealed-inline fixtures and verifies unselected displayed text survives.

Rich-layout tests cover heading metrics with editable markers, table column alignment and wrapping, pointer/caret agreement, visual wrap boundaries, and vertical navigation beside cells of different heights. Headless tests send real mouse press/drag/release events through the custom Iced widget, selecting from a heading through a table and image into a paragraph in both directions, including projection changes that change disclosure between events. Editing tests cover cell navigation, added rows, TSV paste, escaped pipes, formatting across cells, atomic image selection/deletion/undo, and source switching. Image tests exercise local/percent-encoded/file URLs, decoding failures, and a loopback HTTP server with response size limits. The HTTP test requires permission to bind localhost in a sandbox.

Contextual disclosure tests cover innermost nested spans, adjacent spans on the same line, autolinks, closing endpoints, heading prefixes, and semantic styles. Hidden long destinations are checked against an equivalent short destination to verify that they contribute no width or wrapping. Reflow tests verify source-based pointer hits. A headless widget regression checks screen Y anchoring when switching headings or toggling focus at narrow widths, reverse selections, retained drag anchors and preferred X, wrap affinity, and visible-row anchoring when focusing an offscreen caret. Setext underlines remain editable when disclosed.

Earlier desktop builds were checked for active heading marker disclosure, Home followed by inserting `#` to change the level without replacing text, undo, and concealment when moving into a paragraph. Other desktop checks covered live heading sizes, table rendering and cell edits, Tab navigation, local image rendering, drag selection through table/image/text, whole-document selection, source switching, and native save dialogs. The contextual-disclosure and large-document changes are verified by headless model/layout/widget tests; native visual inspection, real IME, screen-reader, and broader platform testing remain necessary.

File safety tests cover exact-byte atomic writes, conflict detection, and an in-flight save completing before later edits. The earlier desktop smoke test also verified native save dialogs and exact Markdown on disk.

## Small model/layout benchmarks

The following samples predate the large-document optimizations and are retained as historical context. Run `mise run bench`. These are single local samples. The rich-layout benchmark includes text shaping/cache reuse and layout geometry, but none includes drawing, event delivery, or display presentation. The three benchmarks may run concurrently, so cold font initialization and shared font locks can affect the initial samples.

| Operation | Measured result |
| --- | ---: |
| Parse/cache 6,000 blocks | 10.1 ms |
| 1,000 source edits with cache reuse, 6,000 blocks | 1,008 ms total; 1.008 ms/edit |
| 100 continuous native-buffer/source/projection edits, 2,000 blocks | 214 ms total; 2.14 ms/edit |
| Initial rich layout, 2,000 blocks | 109 ms |
| 100 syntax disclosure changes + cached rich layouts, 2,000 blocks | 658 ms total; 6.58 ms/update |
| 100 projection rebuilds + cached rich layouts, 2,000 blocks | 558 ms total; 5.58 ms/update |
| Construct the continuous-editor test app | 103 ms |

The continuous fixture contains 1,000 headings and 1,000 paragraphs with bold text and links. Its edit measurement includes the native content mutation, source patch/history, Markdown reparse/cache reuse, projection/highlight rebuild, buffer synchronization, and statistics/search refresh. App construction can include cold font initialization and preference loading. These are not frame times or worst-case latency guarantees.

The old ~3.9 µs block-only edit benchmark no longer describes the active editor. Continuous editing still reparses top-level document structure; safe word edits update projection ranges and styles incrementally.

## Large-document interaction profile

`mise run profile` runs a dedicated release harness serially at 10,000 and 50,000 blocks. It records p50/p95/max input dispatch, App updates, layout, draw-command submission, software rasterization, and their sum. It covers typing at three document positions, disclosure, local and large selections, wheel scrolling, keyboard selection, dragging, and syntax edits that intentionally take the full-parser path. See [methodology, before/after results, and limits](performance.md). These samples include software rendering but exclude OS input delivery, window chrome, GPU work, and display presentation.

## What Iced supplies

The typed state/update model makes background work, file guards, and theme changes testable. One native text buffer provides text mutations and logical motions. Iced/cosmic-text rich paragraphs supply shaped glyphs and hit testing. The live widget adds shared block geometry, selection, clipboard handling, scrolling, and Iced's input-method integration. The stock highlighter alone could not provide variable sizes or embedded objects. GPU rendering and software fallback use the same app state.

## Remaining release work

1. **Scalability.** See the [10k/50k-block profile](performance.md), [viewport virtualization](virtualization.md), [native-buffer initialization](native-buffer.md), and [incremental block parsing](incremental-parsing.md). Rich shaping is limited to resident geometry, native loading defers shaping, and eligible source edits reparse isolated blocks. Global references, containers and uncertain boundaries still take full parsing; native text snapshots, metadata rebasing, outline construction and exceptionally long paragraphs/table rows remain proportional to document size. GPU presentation and native event latency still need direct measurement.
2. **Editing fidelity.** Exercise actual CJK IMEs, bidirectional scripts, grapheme clusters, alternate keyboard layouts, dragging/autoscroll, clipboard interoperability, and composition on all platforms. Some punctuation joins flatten the affected inline style to preserve text under CommonMark's delimiter rules. Partial-style toggles and complex table selections need further coverage.
3. **Rich authoring controls.** Images currently occupy atomic block flow, including inline image syntax, and show GIFs' first frame. Add image properties/resizing, SVG/animation support, table column/row controls, clickable live task controls, and strikethrough drawing. Read remains an optional comparison renderer.
4. **Durability.** Add crash recovery, versioned snapshots, multiwindow sessions, stronger external-change coordination, and filesystem/power-loss testing. Manual atomic save remains the persistence boundary.
5. **Accessibility and integration.** Iced's widgets in this macOS build are not exposed as navigable native accessibility controls. Screen-reader support remains a release blocker. Native menus, document associations, signing/notarization, and platform packages are incomplete.
6. **Markdown completeness.** Footnotes, mathematics, diagrams, image management, rich table editing, and additional exports remain unfinished. The live projection resolves reference-style links document-wide; Read view's separate block renderer does not yet share those definitions.

The editor now has a shared rich layout over the existing continuous input/source model. Profiling real frame latency, memory, and platform input behavior is the next step toward production readiness.
