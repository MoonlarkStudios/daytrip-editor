# Strikethrough

Live and Read views draw strikethrough for parsed `~~deleted text~~` and
`~deleted text~`. Bold, italics, links, inline code, headings, quotations and table
cells keep their existing typography and semantic colors under the strike.
Escaped tildes and tildes inside code remain literal.

Use the Strikethrough toolbar control or **Cmd/Ctrl+Shift+X** to wrap selected
text in `~~`. As with the other inline commands, formatting applies separately
to selected lines and table cells. Selecting the complete contents of an existing
single- or double-tilde span and invoking the command removes its markers.
With no selection, the command inserts `~~deleted text~~` at the caret.

The active inline span reveals its editable markers. Revealed syntax is muted
and has no strike; concealed markers reserve no width. Strikethrough does not
change shaping, caret stops, selection geometry, or document height. Directional
source selections, source switching, and the existing transaction/undo path retain
their normal behavior.

## Implementation and validation

The projection retains the parser's actual delimiter widths and emits a semantic
strikethrough flag. Rich layout builds strike segments from the same measured
glyphs used for text, including wrapping and bidirectional runs, and caches them
with the paragraph. Segments retain their text color and participate in the
existing viewport decoration index. Strikes draw above selection backgrounds and
text, so selecting text retains its formatting. Drawing does not parse Markdown.

Private regressions cover single/double markers, escaped/code literals, nested
styles, Unicode/graphemes and bidirectional text, wrapped geometry, unstruck
disclosed syntax, semantic colors, reverse selection and atomic undo/redo.
An opaque-selection software pixel regression checks strike visibility in both
selection directions.
Incremental and virtual-layout tests compare strike geometry with fresh layout
after edits, disclosure changes, and eviction.

Render the [shared fixture](../tests/fixtures/strikethrough.md) without opening a
native window:

```sh
mise exec -- cargo test --locked --lib strikethrough_previews -- --ignored --nocapture --test-threads=1
```

The command writes concealed, active, and selected previews for all three presets to
`/tmp/dosty-strikethrough-<preset>-<state>.png`. Software previews do not establish
native input-method, screen-reader, or GPU presentation behavior.
