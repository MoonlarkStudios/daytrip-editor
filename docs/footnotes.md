# Footnotes

Use **Footnote** or Cmd/Ctrl+Shift+N to insert a note or edit the reference or
definition at the caret. A new reference follows the selected passage; the
passage stays intact. Its definition is appended to the document. References and
definitions remain ordinary Markdown, for example:

```markdown
A thought worth qualifying[^note-1].

[^note-1]: A **formatted** explanation.

    Another paragraph, still inside this note.

    - A nested list item
```

## References and navigation

Live displays resolved references as compact `[1]`, `[2]`, … tokens in order of
their first source occurrence. Repeated references share a number. Labels use
the same Unicode case folding as the Markdown parser. Editing a reference reveals
its exact `[^label]` syntax. The source selection, direction, caret affinity and
viewport retain their existing meanings through disclosure; concealed label text
reserves no space. Copying a complete reference retains its Markdown syntax.

Cmd/Ctrl-click a Live reference, or click its number in Read, to open its details.
The dialog includes a formatted preview and the editable definition source.
**Go to definition** navigates to its body. **Back to reference** in the status bar
returns to the initiating reference or selection, following unrelated replica
edits. Navigation does not create undo history.

Missing references remain literal. Open their details to add the missing
definition. Duplicate definitions remain intact and are shown as ambiguous;
resolve them directly in the document before applying edits through the dialog.
Unreferenced definitions remain visible and editable.

## Editing and compatibility

Apply uses stable, expected-text-checked source targets. Unrelated edits rebase
them; overlapping reference/body edits, a changed document session, or a newly
occupied generated label leave the dialog open with an error and change no
source. Each accepted insertion or edit is one human transaction and one undo
group. Applying unchanged details creates no edit.

New notes indent continuation lines automatically. Existing definitions are
edited as exact Markdown source, including their continuation indentation; keep
continuation paragraphs and lists indented by four spaces. Preview and Read
remove only the footnote container indentation for presentation. Enclosing quote
and list structure stays intact in Read. The dialog preview is limited to the
first 64 KiB at a UTF-8 boundary; this limit does not truncate source or edits.

Footnotes now use pulldown-cmark's current four-space continuation syntax, replacing
the previously enabled legacy footnote mode. Adjacent definitions are separate
notes, indented paragraphs/lists/code belong to their definition, and undefined
references stay literal. Opening a legacy document preserves its exact source;
its footnote interpretation can change under these documented syntax rules.

The optional Read renderer has no native footnote extension. It receives cached,
display-only ordinary Markdown with the same global numbers and internal preview
links. Global source changes invalidate these cached views so numbers and link
targets cannot retain an earlier source offset. This does not fix Read's separate
reference-style link-definition limitation.

Stock Iced Read and the dialog preview do not reliably style indented code
blocks: the text remains present, but can appear as prose after an empty code
surface. Live retains the correct indented-code metadata and rendering. Fenced
code blocks are the reliable rich-code path in Read and preview. This existing
renderer limitation is separate from footnote continuation handling.

## Validation

The shared [footnote fixture](../tests/fixtures/footnotes.md) exercises repeated
and missing references, Unicode, tables and multiline definitions. Private model
and application regressions compare disclosure and accepted incremental updates
with fresh projections, every UTF-8 source/display boundary, global Read cache
invalidation, escaped labels and literal/code lookalikes, modern continuation
syntax, navigation through replica edits, conflicts, and exact undo/redo.

Run `mise run check`, `mise run check-repo`, and `mise run release`. The software
renderer and synthetic input tests do not establish native IME, screen-reader or
platform readiness; the [release gaps](evaluation.md#remaining-release-work)
remain applicable.

The 2026-10-29 feature-branch validation passed formatting, strict Clippy,
209 ordinary regressions (18 optional tests ignored), repository checks, the
optimized application build, and the dependency audit with the four existing
reviewed maintenance exceptions. `unicase` 2.10.0 is now a direct dependency to
match the parser's label comparison; it was already present at that version in
the lockfile.

Software previews were generated for all three presets in Live, Read, and the
details dialog at 1180×820 and for details at 760×520. Representative evidence:
[Paper Live](validation/footnotes/paper-live.png),
[Paper Read](validation/footnotes/paper-read.png),
[Paper details](validation/footnotes/paper-details.png),
[Paper narrow details](validation/footnotes/paper-details-narrow.png),
[Midnight details](validation/footnotes/midnight-details.png), and
[Sage Live](validation/footnotes/sage-live.png).
The narrow dialog scrolls to reach its lower controls. No native windows or user
documents were opened or changed by this validation.

Reproduce the previews and the optional serial feature benchmark with:

```sh
mise exec -- cargo test --locked footnote_ui_previews -- --ignored --nocapture --test-threads=1
mise exec -- cargo test --locked --release performance_footnotes_parse_and_read -- --ignored --nocapture --test-threads=1
```

The benchmark measures model/projection construction and cold/warm Read item
caching at 1,000 and 5,000 notes over 30 samples. It excludes layout, rendering,
native input, IO, and GPU/display presentation. Its timings are observations,
never ordinary regression assertions. Combined release interaction profiles are
collected separately from this feature-specific harness.
