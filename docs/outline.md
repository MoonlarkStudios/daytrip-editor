# Outline navigation and section moves

The Outline sidebar lists document headings as a hierarchy. Disclosure triangles hide descendant rows in the outline only; the writing surface and Markdown remain unchanged. Collapse/expand all controls are available above the tree. The current section remains highlighted while editing its body, and a collapsed ancestor represents a hidden current child. Fold state is local to the open document session and resets when another document opens.

## Navigation

- Click a heading to jump to it. Read view stays in Read mode; Live and Source retain their mode.
- Cmd/Ctrl+Shift+J opens Quick Jump, including when the sidebar is hidden. Search is case-insensitive, matches whitespace-separated terms in heading titles and ancestor titles, and ranks title matches before ancestor-only matches. Duplicate titles show their immediate parent.
- Up/Down selects a result, Enter jumps, and Escape cancels without changing the source selection. Jumping expands the target's ancestors. The picker shows a bounded window around the selected result.
- Cmd/Ctrl+Alt+Left collapses the current branch, or its parent if the current heading has no children or is already collapsed. Cmd/Ctrl+Alt+Right expands it. These commands affect the outline only.

## Rearranging

Drag the grip on the right of a heading row. The upper and lower halves of a target row mean before and after that target's **whole section**, respectively. An accent line marks a valid destination. Targets must have the same heading level; a move can cross parents, but never rewrites heading markers. Wheel scrolling remains available during a drag. Releasing outside a row, Escape, loss of window focus, or a source revision cancels the drag.

Cmd/Ctrl+Alt+Up/Down moves the section containing the caret before/after its previous/next sibling with the same parent and level. A section contains its heading, body, nested sections, and separating whitespace, ending immediately before the next heading of equal or lower level. Preamble text before the first heading stays in place. ATX and Setext headings are supported. Headings inside lists, quotations, HTML, or fenced code are excluded.

Each accepted move is one human undo group, separate from typing before and after it. A caret or selection inside moved text follows those bytes. Other endpoints rebase through the deletion/insertion; selections spanning reordered sections retain their anchor/head direction. Undo restores the original source and selection; redo restores the moved selection. Fold state follows locally moved headings through undo/redo, including duplicate titles.

The operation preserves existing bytes and line endings, adding only missing blank-line separation at the insertion boundaries. Undo removes that added separation. Before committing, the model checks that the candidate parses with the expected heading positions and levels. A move that would let an unfinished code fence or HTML block swallow another section is refused with an explanation. Invalid, adjacent no-op, and self-drops do not create undo entries. Existing transaction size limits, file guards, and dirty/recovery handling apply.

## Validation

Private module tests cover hierarchy, nested/fake headings, Unicode and reverse selection, CRLF, indentation, Setext, missing final newlines, unsafe moves, cross-parent moves, isolated undo groups, fold anchors through remote prefix edits and deletion/undo, stale drag cancellation, loading/modal guards, and query/cancel behavior. Full-widget tests send mouse and keyboard events through the application tree in Live and Source modes, including the minimum 760×520 window size.

Run `mise run check` and `mise run release`. Run `mise exec -- cargo test --locked outline_ui_previews -- --ignored` to render the full sidebar and light/dark picker into ignored `target/outline-validation/` files. These software-rendered checks do not establish native OS drag delivery, IME behavior, screen-reader support, or GPU presentation latency.

## Release measurements

Measured on 2026-10-25 on an Apple M2 Max, 32 GiB RAM, macOS 26.7.1 (25G241), aarch64, mise Rust 1.95.0, release/thin LTO/codegen-units=1. [Environment](performance/outline/environment.txt) and [source checksums](performance/outline/source-sha256.txt) identify this run. This is a feature baseline, not a matched before/after performance comparison.

```sh
mise exec -- cargo test --locked --release performance_outline_sections -- --ignored --nocapture --test-threads=1
DAYTRIP_PROFILE_SAMPLES=10 mise run profile
```

The dedicated fixture has ten blocks per chapter: a parent heading, introductory paragraph, child heading, and seven paragraphs containing formatting, a link, and Unicode. The 10k/50k cases contain 2,000/10,000 headings and 926,010/4,670,010 source bytes. Query timings use ten distinct queries. Move/undo timings use five repetitions moving the first complete chapter to the end, restoring exact source after each undo. These times include the application command, CRDT transaction, source parsing, outline refresh, projection, and native buffer synchronization; they exclude widget construction/layout, drawing, OS/GPU delivery, and file IO.

| Operation | 10k blocks, median / maximum ms | 50k blocks, median / maximum ms |
| --- | ---: | ---: |
| Heading query (n=10) | 0.276 / 0.397 | 1.407 / 1.478 |
| Move whole section (n=5) | 20.924 / 42.302 | 127.454 / 138.560 |
| Undo section move (n=5) | 17.227 / 19.348 | 108.950 / 111.230 |

Collapse-all and expand-all were single observations: 0.538/0.012 ms at 10k, and 2.276/0.060 ms at 50k. Application construction/load was 154.756 ms for the first 10k case and 173.374 ms for the subsequent 50k case. Those single loads include application initialization and different process-global warm state, so they do not establish cold file-open latency.

The existing large-document interaction profile passed at both sizes with ten samples per repeated interaction, including selection, disclosure, scrolling, IME commit, and undo/redo. It uses the separate mixed-content fixture documented in [performance.md](performance.md), includes editor layout and software rasterization, and excludes the sidebar/window shell. Middle typing measured 2.129/2.941 ms median/maximum at 10k and 6.012/9.431 ms at 50k. End typing had a 118.659 ms maximum at 50k; document-wide structural work remains a limit. These small sample counts and CPU/software paths are not native frame-time or worst-case guarantees.

Raw evidence: [outline benchmark](performance/outline/benchmark.txt), [interaction profiles](performance/outline/profile.txt), [full checks](performance/outline/check.txt), [release build](performance/outline/release.txt), and [preview generation](performance/outline/previews.txt). No timing assertions were added to ordinary tests.
