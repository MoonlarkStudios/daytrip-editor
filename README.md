# Daytrip

A quiet, native Markdown editor built with **Iced 0.14** and Rust. Warm paper, considered typography, a restrained sidebar, and one surface for writing and reading.

Daytrip is a working technology evaluation toward a Typora-like editor. It uses production-oriented file handling and tested document state, but **is not yet a production-ready Typora replacement**. Continuous editing includes rich heading sizes, editable tables, images, viewport virtualization, deferred native-buffer shaping, conservative incremental block parsing, reusable native text snapshots, and in-place source/projection updates. Broader editing fidelity, remaining linear document work, broader platform validation, and accessibility remain release work.

## Run

Install [mise](https://mise.jdx.dev/getting-started.html) and the platform's native build tools, then:

```sh
mise trust
mise install
mise run dev
```

Rust **1.95.0**, rustfmt, and Clippy are pinned in `mise.toml`. No separate global Rust configuration is needed. The first build downloads crates; subsequent builds use `Cargo.lock`.

```sh
mise run check                 # rustfmt, Clippy with -D warnings, tests
mise run check-repo            # documentation links, unused dependencies, CI and audit policy
mise run audit                 # current RustSec findings and reviewed exceptions (Python 3.9+)
mise run check-secrets         # credential scan of full reachable Git history
mise run release               # optimized desktop executable
mise run bench                 # optimized model and layout benchmarks
mise run profile               # 10k/50k-block events through software rendering
mise exec -- cargo run --locked -- /path/to/note.md
mise exec -- bash scripts/bundle-macos.sh  # target/Daytrip.app
```

Set `STILL_CONFIG_DIR` to use an isolated preferences directory when testing multiple builds. The internal Cargo package and build executable retain the name `still`; existing configuration locations and collaboration formats remain compatible with earlier builds.

macOS requires Xcode Command Line Tools. Linux needs a C toolchain and the X11/Wayland development libraries (on Debian/Ubuntu: `build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-xcb-dev`). Linux file dialogs require `libdbus`, a GTK/GNOME/KDE desktop portal backend, and Zenity for fallback. Windows needs the MSVC C++ build tools. Only macOS has been exercised locally; the CI matrix is configured for all three.

## Write

Open **The quiet hour · sample** in the sidebar for a complete, editable document with a bundled photograph, caption, links, and attributed quotation.

- **Live editor:** the active heading reveals its prefix, and only the innermost inline span under the caret reveals its delimiters or link destination. Other completed syntax occupies no space. Moving out of a span conceals it immediately; leaving the editor conceals all markers. Small local reflow is allowed, with source-based caret/selection preservation and viewport anchoring. Arrow keys, mouse dragging, copy/cut/paste, and selection operate across the entire document.
- **Rich blocks in Live:** headings have six size levels; tables have editable, wrapping cells and aligned columns; images render in the page. Cursor movement and selection stay continuous through all of them. Read remains an optional comparison view.
- **Tables:** Tab/Shift+Tab moves between cells. Enter moves down the current column and adds a row at the end. Tab from the last cell also adds a row. Shift+Enter creates a paragraph after the table. Pasting TSV or multiple lines fills cells and grows rows; literal pipes are escaped. Backspace/Delete at a cell boundary navigates across the boundary.
- **Images:** paste a copied image file or raster image, drop PNG/JPEG/GIF/WebP files, or use Image to choose a file or URL. Local imports use immutable assets beside your Markdown. Double-click an image or use Image to edit alt text, caption, and photo credit. Images fit the page; delete, replace, copy/cut, and undo preserve their Markdown. Save As copies relative assets; HTML export embeds them.
- **Links and credits:** Cmd/Ctrl+K creates or edits a link; pasting a URL onto selected text links it. Normal clicks edit and Cmd/Ctrl-click opens the destination. Use ❝ for attributed quotations. Captions, photo credits and source references remain ordinary Markdown.
- **Source:** use the bottom-left toggle or Cmd/Ctrl+Shift+M to inspect the exact Markdown. Switching preserves the caret and selection, including selection direction.
- **Formatting:** select text and use Cmd/Ctrl+B or I, or the contextual toolbar. Code, links, headings, blockquotes, lists, tasks, fences, and tables can also be inserted.
- **Lists and tasks:** Enter continues lists and resets the next task to unchecked; Enter in an empty item exits the list. Edit disclosed heading markers directly with ordinary selection, Backspace, and Delete. Click task checkboxes in Read view to toggle them.
- **Navigation:** switch the sidebar to Outline to jump to headings, collapse branches, or expand/collapse all. Cmd/Ctrl+Shift+J searches all headings, including collapsed branches; use Up/Down, Enter, and Escape. Drag a heading's grip before or after another heading of the same level to move its entire section, including child sections. Cmd/Ctrl+Alt+Up/Down moves the current section between siblings; Cmd/Ctrl+Alt+Left/Right collapses/expands its outline branch. Moves preserve heading levels and undo in one step. Alt+Up/Down still moves between blocks. See [outline behavior and validation](docs/outline.md).
- **Find and replace:** Cmd/Ctrl+F, with exact, case-sensitive Unicode matching. Enter/Next reveals and selects a match.
- **Files:** Cmd/Ctrl+N, O, S; Cmd/Ctrl+Shift+S for Save As. Dropped Markdown files open through the same unsaved-change guard; images insert at the current selection. Export creates a standalone HTML document.
- **Focus:** Cmd/Ctrl+Shift+F hides the sidebar. Cmd/Ctrl plus/minus adjusts text size. Cmd/Ctrl+Z and Shift+Z undo and redo.

Changes are saved manually. Closing, opening, or replacing a dirty document requires Save, Discard, or Cancel. Saves use a synchronized temporary file in the destination directory, atomic replacement, existing permissions, and a check against the last saved contents to detect external modifications. Edits made while saving remain dirty. After two idle seconds, or every 30 seconds of continuous writing, unsaved work receives a private, checksummed recovery checkpoint. Recovery copies in the sidebar opens a compatible editable copy with the saved-file conflict baseline intact. Save valuable work regularly: the newest work after the last checkpoint may not survive a crash. See [writing workflows, asset storage, recovery and export](docs/writing-workflows.md).

## Themes

Open **Appearance & shortcuts** to select Paper, Midnight, or Sage, or load a TOML theme. Copy `themes/paper.toml` as the starting point. The original semantic `#RRGGBB` color tokens remain required; `brand_accent` is optional and falls back to `accent`. Paper pairs warm paper with plum ink; Midnight uses warm plum surfaces; Sage offers linen and quiet greens. See the [Daytrip appearance system](docs/design-system.md). Unknown keys, invalid colors, and out-of-range typography produce an error while the previous theme remains active.

```text
Built-in preset or user TOML
             ↓
ResolvedTheme { semantic Tokens, Typography, iced::Theme }
             ↓
App.resolved_theme
             ↓
Central widget styles, Markdown rendering, and canvas icons
```

Themes and preset previews are resolved at startup or theme selection, never in `view` or `.theme(...)`. `App::theme` returns a clone of the stored Iced theme handle. Components and canvases receive the same tokens. Palette-derived Iced defaults use those colors too. The selected theme and recent paths persist in the platform's application configuration directory.

Typography supports `body_size` (12–32), `line_height` (1.2–2.2), and `content_width` (480–1200). Newsreader regular, bold, italic, and bold italic are bundled under the SIL Open Font License for consistent rendering.

## Engineering

See [CONTRIBUTING.md](CONTRIBUTING.md) for source boundaries, validation commands, Rust conventions, and commit policy. [AGENTS.md](AGENTS.md) provides concise instructions for agent work. The [documentation index](docs/README.md) separates current behavior and architecture from historical measurements. [Evaluation and release gaps](docs/evaluation.md) records remaining work, and [SECURITY.md](SECURITY.md) describes reporting and dependency review.

Current support includes headings, inline styles, links, nested lists, tasks, blockquotes, rules, GFM tables, code fences, and PNG/JPEG/GIF/WebP images. Reference-style links resolve across blocks in the live projection. Image objects use block layout even when written inline; GIFs show their first frame. Strikethrough lines and clickable task controls remain in Read view. LaTeX math, Mermaid, footnote popovers, image resize/crop controls, column/row manipulation controls, typewriter scrolling, PDF/office exports, tabs, and folder watching remain unfinished.

MIT licensed. The bundled font has its own [license](assets/fonts/OFL.txt).

Offline replicas and versioned agent proposals are available under **Appearance → Collaboration…**. Markdown stays an ordinary `.md` file; keep its hidden `.still` checkpoint alongside it to retain shared history. See [collaboration and recovery](docs/collaboration.md) and [CRDT measurements](docs/crdt-performance.md).

The **daytrip · open book** identity is available in the [branding kit](branding/daytrip/README.md), including editable SVG masters, logo variants without a tagline, platform app icons, palette, and typography guidance. The application and macOS bundle use the Daytrip name and book symbol.
