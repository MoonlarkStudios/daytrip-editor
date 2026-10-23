# Still

A quiet, native Markdown editor built with **Iced 0.14** and Rust. Warm paper, considered typography, a restrained sidebar, and one surface for writing and reading.

Still is a working technology evaluation toward a Typora-like editor. It uses production-oriented file handling and tested document state, but **is not yet a production-ready Typora replacement**. Continuous editing includes rich heading sizes, editable tables, images, viewport virtualization, deferred native-buffer shaping, conservative incremental block parsing, reusable native text snapshots, and in-place source/projection updates. Broader editing fidelity, remaining linear document work, recovery, and accessibility remain release work.

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
mise run release               # optimized desktop executable
mise run bench                 # optimized model and layout benchmarks
mise run profile               # 10k/50k-block events through software rendering
mise exec -- cargo run --locked -- /path/to/note.md
mise exec -- bash scripts/bundle-macos.sh  # target/Still.app
```

Set `STILL_CONFIG_DIR` to use an isolated preferences directory when testing multiple builds.

macOS requires Xcode Command Line Tools. Linux needs a C toolchain and the X11/Wayland development libraries (on Debian/Ubuntu: `build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-xcb-dev`). Windows needs the MSVC C++ build tools. Only macOS has been exercised locally; the CI matrix is configured for all three.

## Write

- **Live editor:** the active heading reveals its prefix, and only the innermost inline span under the caret reveals its delimiters or link destination. Other completed syntax occupies no space. Moving out of a span conceals it immediately; leaving the editor conceals all markers. Small local reflow is allowed, with source-based caret/selection preservation and viewport anchoring. Arrow keys, mouse dragging, copy/cut/paste, and selection operate across the entire document.
- **Rich blocks in Live:** headings have six size levels; tables have editable, wrapping cells and aligned columns; images render in the page. Cursor movement and selection stay continuous through all of them. Read remains an optional comparison view.
- **Tables:** Tab/Shift+Tab moves between cells. Enter moves down the current column and adds a row at the end. Tab from the last cell also adds a row. Shift+Enter creates a paragraph after the table. Pasting TSV or multiple lines fills cells and grows rows; literal pipes are escaped. Backspace/Delete at a cell boundary navigates across the boundary.
- **Images:** local paths, file URLs, and HTTP/HTTPS URLs load and decode in background tasks. Relative paths resolve beside the saved document. Images fit the page and behave as single selectable objects; delete, replace, copy/cut, and undo preserve their Markdown.
- **Source:** use the bottom-left toggle or Cmd/Ctrl+Shift+M to inspect the exact Markdown. Switching preserves the caret and selection, including selection direction.
- **Formatting:** select text and use Cmd/Ctrl+B or I, or the contextual toolbar. Code, links, headings, blockquotes, lists, tasks, fences, and tables can also be inserted.
- **Lists and tasks:** Enter continues lists and resets the next task to unchecked; Enter in an empty item exits the list. Edit disclosed heading markers directly with ordinary selection, Backspace, and Delete. Click task checkboxes in Read view to toggle them.
- **Navigation:** switch the sidebar to Outline. Alt+Up/Down moves between blocks. The sidebar also provides recent documents and an editable Markdown guide.
- **Find and replace:** Cmd/Ctrl+F, with exact, case-sensitive Unicode matching. Enter/Next reveals and selects a match.
- **Files:** Cmd/Ctrl+N, O, S; Cmd/Ctrl+Shift+S for Save As. Dropped files open through the same unsaved-change guard. Export creates a standalone HTML document.
- **Focus:** Cmd/Ctrl+Shift+F hides the sidebar. Cmd/Ctrl plus/minus adjusts text size. Cmd/Ctrl+Z and Shift+Z undo and redo.

Changes are saved manually. Closing, opening, or replacing a dirty document requires Save, Discard, or Cancel. Saves use a synchronized temporary file in the destination directory, atomic replacement, existing permissions, and a check against the last saved contents to detect external modifications. Edits made while saving remain dirty. There is no crash recovery yet; save valuable work regularly.

## Themes

Open **Appearance & shortcuts** to select Paper, Midnight, or Sage, or load a TOML theme. Copy `themes/paper.toml` as the starting point. Every color is a required, semantic `#RRGGBB` token. Unknown keys, invalid colors, and out-of-range typography produce an error while the previous theme remains active.

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

See [CONTRIBUTING.md](CONTRIBUTING.md) for source boundaries, validation commands, Rust conventions and project exceptions, and commit policy. [AGENTS.md](AGENTS.md) provides concise instructions for agent work. The code follows the applicable [Google Rust style guidance](https://google.github.io/styleguide/rustguide.html). Technical details are in [architecture](docs/architecture.md), [evaluation notes](docs/evaluation.md), the [large-document baseline](docs/performance.md), [viewport virtualization](docs/virtualization.md), [native-buffer initialization](docs/native-buffer.md), [incremental block parsing](docs/incremental-parsing.md), [native snapshots](docs/native-snapshots.md), and [source/projection updates](docs/source-projection-updates.md).

Current support includes headings, inline styles, links, nested lists, tasks, blockquotes, rules, GFM tables, code fences, and PNG/JPEG/GIF/WebP images. Reference-style links resolve across blocks in the live projection. Image objects use block layout even when written inline; GIFs show their first frame. Strikethrough lines and clickable task controls remain in Read view. LaTeX math, Mermaid, footnote popovers, image property/resize controls, column/row manipulation controls, typewriter scrolling, PDF/office exports, tabs, and folder watching remain unfinished.

MIT licensed. The bundled font has its own [license](assets/fonts/OFL.txt).

Offline replicas and versioned agent proposals are available under **Appearance → Collaboration…**. Markdown stays an ordinary `.md` file; keep its hidden `.still` checkpoint alongside it to retain shared history. See [collaboration and recovery](docs/collaboration.md) and [CRDT measurements](docs/crdt-performance.md).
