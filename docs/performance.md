# Large-document performance baseline

Measured locally on 2026-10-22 with Rust 1.95.0, Iced 0.14, macOS/Apple silicon, release optimization, and the bundled Newsreader fonts. This records the current validated editor before organizational refactoring.

## Reproduce

```sh
mise run check
mise run profile
STILL_PROFILE_BLOCKS=10000 STILL_PROFILE_SAMPLES=60 mise run profile
STILL_PROFILE_BLOCKS=50000 STILL_PROFILE_SAMPLES=30 mise run profile
```

The ignored profile runs serially and defaults to 30 samples at 10,000 and 50,000 blocks. Fixtures contain headings, styled paragraphs, links, lists/tasks, quotes, code fences, tables, and atomic image placeholders. Their sizes are 910,450 and 4,574,450 source bytes. No network requests or image decoding run; image tasks are dropped. The harness asserts block counts and that real keyboard events actually insert the expected characters.

The profile separates widget event dispatch, App updates, rich layout, draw-command submission, software rasterization, and their sum. The 720×800 viewport uses a persistent Iced widget tree and software renderer. Typing and wheel events pass through the widget. Disclosure and direct local/large selections use source-selection messages; keyboard selection and dragging are also measured separately. Fonts are loaded before timing.

These are CPU paths through software rasterization, not native input-to-photon latency. Window chrome, outline construction, OS event delivery, GPU execution, display presentation, file I/O, and decoded-image memory are excluded. Cold load and first layout are separate single samples. Do not add independently reported phase percentiles to estimate a total percentile; totals are calculated per interaction.

## Before and after

Values below are median milliseconds per interaction, including software rasterization. Before: 30 samples at 10k, only 3 at 50k because the original table lookup made each end-of-document keypress take about 26 seconds. After: 30 samples at each size. The 50k baseline is directional evidence; its p95 is merely the maximum of three samples.

| Interaction | 10k before | 10k after | 50k before | 50k after |
| --- | ---: | ---: | ---: | ---: |
| typing/start | 64.13 | 7.51 | 355.62 | 39.89 |
| typing/middle | 438.36 | 7.85 | 10164.58 | 41.10 |
| typing/end | 1087.89 | 7.79 | 26349.81 | 41.87 |
| disclosure | 49.69 | 3.36 | 262.24 | 16.28 |
| selection/local | 2.71 | 1.61 | 9.96 | 3.66 |
| selection/half-document | 4.21 | 1.69 | 20.36 | 3.78 |
| scroll | 3.12 | 1.24 | 10.51 | 1.23 |

The main input defect was a table-cell scan repeatedly converting the native caret through the whole text. Table/row lookup now converts once and uses sorted ranges. Cached local caret stops avoid reprocessing every glyph after edits. Ordinary word edits and interior spaces can rebase projection ranges and share unaffected highlight arrays. Disclosure repaints affected lines. Paragraph patches reshape only affected ordinary paragraphs and rebase exact geometry below them. Visible interval queries avoid submitting offscreen selection quads. Opening a document initializes its native buffer directly instead of pasting the entire file into an existing buffer.

## Final distribution and remaining costs

All values below are milliseconds, p50 / p95 / max, from the final 30-sample run.

| Interaction | 10k | 50k |
| --- | ---: | ---: |
| typing/middle | 7.85 / 8.72 / 8.80 | 41.10 / 42.69 / 43.52 |
| typing/end | 7.79 / 8.62 / 24.60 | 41.87 / 46.83 / 147.22 |
| disclosure | 3.36 / 3.56 / 3.59 | 16.28 / 17.09 / 18.95 |
| selection/keyboard | 1.62 / 9.46 / 10.23 | 4.20 / 57.41 / 57.97 |
| selection/drag | 1.74 / 9.81 / 9.86 | 5.00 / 57.47 / 59.65 |
| scroll | 1.24 / 1.67 / 1.71 | 1.23 / 1.58 / 1.78 |
| typing/syntax-fallback | 25.01 / 26.57 / 26.95 | 144.30 / 151.34 / 157.32 |

Load changed from 1.857635125s to 597.664209ms at 10k, and from 39.304416959s to 2.88942125s at 50k. Initial shaping still covers the entire document: about 306 ms at 10k and 1530 ms at 50k, including the first software draw.

The serial command reported maximum resident memory of 2,451,931,136 bytes (about 2.28 GiB) across its process tree. This is a command-level peak over both fixtures, not steady-state memory for one window. Retained paragraphs, native text storage, and geometry remain material costs; this change does not implement viewport virtualization.

The fast paths are intentionally conservative. Structural edits, delimiter changes, unresolved references/autolinks, Unicode changes, table/image relayout, and multiline markers can require full parsing or layout. The syntax-fallback case repeatedly inserts underscores to exercise that path. Selection that changes disclosure across table/image boundaries can also trigger full layout. The observed 50k structural-edit latency and retained memory are the clearest next targets: incremental top-level parsing and viewport virtualization, followed by native/GPU presentation measurements.

## Correctness boundaries

All source selections remain UTF-8 byte offsets with distinct anchor/head direction. Disclosure keeps its source-based viewport anchor and wrap affinity. Differential tests compare accepted projection edits and every source/display mapping with full parsing; disclosure styles are compared at every caret in mixed fixtures. Incremental layout is compared with fresh layout through repeated reflow, including headings, links, tables and images. Existing undo, reverse-selection, cross-block dragging, clipboard, IME commit, file-safety and theme tests remain in `mise run check`.

No internal APIs were exposed for integration tests. Unit tests stay next to the implementations; the profile is a private child of the application module. Benchmark timings are observations, not timing assertions in ordinary tests.

## Raw evidence

- [10k before](performance/before.txt): intentionally interrupted after its complete 10k series; its footer records SIGINT, not a regression failure.
- [50k before](performance/before-50k.txt): three samples per interaction, completed successfully.
- [First optimization pass](performance/intermediate.txt).
- [Second optimization pass](performance/second-pass.txt).
- [Final profile](performance/after.txt): 30 samples per interaction at both sizes, completed successfully; includes process-resource output.
