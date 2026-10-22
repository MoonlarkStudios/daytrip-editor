# The Markdown field guide

Simple marks. Beautiful words. The active heading shows its markers, and the inline span under your caret reveals its syntax. Move out of that span to conceal its marks immediately. Hidden marks leave no gaps; nearby text may reflow while the source caret, selection, and viewport remain anchored. Headings, tables, and images render directly in this live editor. Use **Source** to inspect the exact Markdown.

## The essentials

Use **bold** for emphasis, *italics* for a softer voice, and ~~strikethrough~~ for a change of mind. Put `inline code` between backticks.

## Structure your thoughts

Start a heading with `#`, `##`, or `###`. Click into a heading to edit its markers while keeping the title's size. Home moves to the start of its visible markers. Separate paragraphs with a blank line. Use ordinary arrows and Shift+arrows to move and select across the whole document. Enter continues a list; Enter in an empty item exits it. Alt+Up and Alt+Down jump between blocks. Escape closes find or a dialog.

> A blockquote makes a little space for another voice.

1. An ordered list
2. Another thought
   - With a nested idea

- [ ] An unfinished task
- [x] A little progress

## Show your work

```rust
fn main() {
    println!("Make room for what matters.");
}
```

| Shortcut | Action |
| :--- | :--- |
| Cmd/Ctrl + S | Save document |
| Cmd/Ctrl + F | Find and replace |
| Cmd/Ctrl + B | Bold selection |
| Cmd/Ctrl + I | Italic selection |
| Cmd/Ctrl + Shift + M | Toggle source |
| Cmd/Ctrl + Shift + F | Focus mode |
| Cmd/Ctrl + Z | Undo |
| Cmd/Ctrl + Shift + Z | Redo |

## Tables and images, without leaving the page

Edit a table cell directly. Tab and Shift+Tab move between cells. Enter moves down the current column; Enter or Tab at the end adds a row. Shift+Enter starts a paragraph after the table. Paste tab-separated values to fill several cells at once.

Images render as single selectable objects. Type an image such as `![A quiet room](images/room.png)` using a local path or a web URL. Save your document before using relative paths. Select an image and delete, replace, or copy it; undo restores its original Markdown.

## A note on this experiment

Still is an Iced technology evaluation. Live editing uses one continuous native buffer, with inline syntax projected out of the display and mapped back to the original source. A shared rich layout supplies heading sizes, table cells, and image objects while preserving document-wide selection. Mathematics, Mermaid, rich table manipulation, and PDF export are future work.
