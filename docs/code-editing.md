# Code editing and rendering

Validated on 2026-10-24. Code blocks share the document's focus, selection, source mappings, and undo history. Their controls are separate from the editable text.

## Behavior

- Live code blocks have a language header, Copy control with confirmation, monospace typography, soft wrapping, full-height blank lines, and a subtle active-line background. Empty fences offer a full-height content row with a “Start coding…” prompt.
- Inline code uses smaller monospace text and a background measured from the shaped spans. Concealed backticks reserve no width.
- Enter retains indentation, increases it after an opening bracket, and separates matching brackets onto their own lines. Python/YAML colons also continue indentation. Tab inserts to the next indentation stop; selections indent as complete lines, and Shift-Tab removes indentation. Backspace in leading spaces returns to the previous stop. Existing indentation is preferred; defaults are two or four spaces according to language. Complex Markdown containers retain their native edit handling.
- Empty-block typing, paste, and IME commits insert content before the closing fence while keeping that fence on its own line. Reverse selections and atomic undo/redo retain source coordinates.
- Copy controls return CommonMark's normalized code content without fences or quote/list indentation. Copying a complete code block as part of a rich selection retains its Markdown fences. Copying a fragment returns the selected text.
- Moving into fence syntax reveals that fence for literal editing, including the language. The content position immediately before a closing fence stays available for code entry. Source mode remains available for inspecting all syntax.
- Read view uses the same semantic surface and typography, syntax colors adjusted for contrast, and a Copy button. Its Markdown representation is created lazily and invalidated with the edited source block.

## Highlighting and bounds

Live highlighting uses the existing Iced grammar highlighter. Parser snapshots and preceding runs survive small edits. A changed line invalidates later runs, since a comment or string delimiter can change the styles of otherwise unchanged text. Highlighting advances only through requested resident lines; shaped paragraph keys include the resulting syntax styles.

The grammar cache holds at most 32 entries and 512 KiB of code text. Live blocks over 256 KiB, blocks with a line over 8,192 bytes, and unlabeled blocks use semantic monochrome text. These limits affect coloring; source editing, selection, wrapping, and copy remain available. The optional Read renderer retains its existing separate parser/highlighter. This adds grammar coloring and editing ergonomics; completion, diagnostics, and language servers are outside this change.

Fence boundaries come from CommonMark text-event offsets rather than a second fence scanner. This preserves longer delimiters, shorter literal backtick runs, unclosed fences, CRLF, and code in containers. Parser-normalized clipboard content is shared between projection snapshots. Incremental projection updates rebase both source and display ranges.

## Validation

`mise run check` passed: 167 tests passed and 13 profiling/preview tests were ignored. `mise run release`, `mise run bench` (eight benchmarks), the serial code benchmark, and 10k/50k-block profiles passed. Regression coverage includes grammar snapshots versus fresh highlighting, edits before the parsed frontier, multiline comments, Unicode, empty-block IME commits, reverse indentation selections, undo/redo, full-height empty rows, wrapped caret/hit geometry, normalized Copy controls, lazy Read-cache invalidation, and code geometry after viewport eviction.

The software renderer was inspected in Live and Read modes for all three presets using [the shared fixture](../tests/fixtures/code-editing.md). Previews: [Paper Live](validation/code-editing/live-paper.png), [Midnight Live](validation/code-editing/live-midnight.png), [Sage Live](validation/code-editing/live-sage.png), [Paper Read](validation/code-editing/read-paper.png), [Midnight Read](validation/code-editing/read-midnight.png), [Sage Read](validation/code-editing/read-sage.png). These are widget renders without native window chrome. Existing windows and documents were not opened or modified. Platform IME behavior and accessibility still need native testing beyond the synthetic commit and geometry coverage.

## Measurements

Apple M2 Max, 32 GiB RAM, macOS 26.7.1 (25G241), aarch64, mise Rust 1.95.0, Iced 0.14, release optimization with thin LTO and one codegen unit. The 720×800 software renderer includes input dispatch, application update, layout, drawing, and rasterization; OS delivery, GPU/display presentation, file IO, and image decoding are excluded. Timings are observations, never regression-test assertions.

The dedicated benchmark types 20 characters in the middle of a 2,000-line Rust block. The comparison below isolates the initially implemented eager highlighting from the final lazy Read cache and incremental Live grammar cache. **The eager column is an intermediate implementation of this feature, not the original repository baseline.** Both runs used the same fixture and standalone serial test process.

| Phase | Eager median | Final median | Final p95 / max |
| --- | ---: | ---: | ---: |
| Application update | 147.47 ms | 0.41 ms | 0.55 / 1.04 ms |
| Layout | 145.68 ms | 5.59 ms | 6.99 / 7.13 ms |
| Total, including rasterization | 295.82 ms | 7.98 ms | 10.09 / 11.18 ms |

The final viewport retained 111 fragments and 111 visual rows. Syntax state may cover preceding lines to establish lexical context; the offscreen suffix is deferred.

The standard mixed-document profiles compare the repository state before this change with the final implementation, using identical fixtures and ten samples per interaction. Values are total median milliseconds unless marked as single cold samples.

| Measurement | 10k before | 10k final | 50k before | 50k final |
| --- | ---: | ---: | ---: | ---: |
| Document load, n=1 | 155.10 | 42.00 | 689.09 | 216.16 |
| First frame, n=1 | 11.53 | 29.52 | 24.38 | 43.15 |
| Typing/middle | 2.21 | 2.31 | 6.77 | 5.79 |
| Typing/end | 1.88 | 1.99 | 5.62 | 5.43 |
| Scroll | 1.48 | 1.55 | 1.56 | 1.42 |

The first frame now includes initialization of Live syntax highlighting; Read syntax initialization previously occurred during document load. The warm differences are small and mixed, so these samples do not establish a general throughput improvement. Ten samples give weak tail estimates: the 50k end-of-document structural sample still reached 114.25 ms, compared with 109.28 ms before. That existing fallback remains outside a universal frame-time guarantee. Cold values are single observations; process RSS was unavailable inside the profile sandbox. The general benchmark task runs its ignored tests concurrently; the standalone code report above is the timing comparison for long code.

Reproduce:

```sh
mise run check
mise run release
mise run bench
mise exec -- cargo test --locked --release performance_code_editor -- --ignored --nocapture --test-threads=1
STILL_PROFILE_BLOCKS=10000 STILL_PROFILE_SAMPLES=10 mise run profile
STILL_PROFILE_BLOCKS=50000 STILL_PROFILE_SAMPLES=10 mise run profile
mise exec -- cargo test --locked --lib code_surface_previews -- --ignored --nocapture --test-threads=1
```

Saved logs normalize trailing whitespace.

Raw evidence: [code eager](performance/code-editing/code-eager.txt), [code incremental](performance/code-editing/code-incremental.txt), [10k before](performance/code-editing/before-10k.txt), [10k final](performance/code-editing/after-10k.txt), [50k before](performance/code-editing/before-50k.txt), [50k final](performance/code-editing/after-50k.txt), [benchmarks](performance/code-editing/bench.txt), [checks](performance/code-editing/check.txt), [release](performance/code-editing/release.txt), [previews](performance/code-editing/previews.txt), [source hashes](performance/code-editing/source-sha256.txt), [executable hashes](performance/code-editing/executables-sha256.txt).
