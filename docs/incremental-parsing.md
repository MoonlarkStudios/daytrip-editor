# Incremental block parsing

Measured on 2026-10-22 with mise's Rust 1.95.0, locked Iced 0.14 / pulldown-cmark 0.12, release optimization, macOS 26.7.1 / arm64, and bundled Newsreader fonts. The baseline is `bbbeb0f`, after [deferred native initialization](native-buffer.md).

## Parsing boundaries

Previously, every source edit discovered all top-level Markdown ranges. Unicode input and formatting changes also took the full projection parser, and undo/redo repeated the source parse after applying its patch.

The source model now certifies isolated windows for unindented paragraphs, headings, tables and rules. An edit must stay inside the old block; blank lines on either side remain untouched, neighboring blocks must be safe, and both old/new windows must fit 64 KiB. The new fragment is parsed before accepting it. It may produce multiple independent blocks, allowing paragraph splits and local topology changes. Unchanged block rendering stays owned by the model; suffix offsets and statistics are adjusted without rediscovering other blocks.

The boundary rules deliberately cover less than all Markdown. Containers, HTML, fences, indentation, colon paragraphs, definitions, footnotes, unsafe neighbors, cross-block edits, gaps and large windows retain the full-parser path. Documents with reference definitions or footnotes use full source parsing, preserving document-wide resolution. Definition lists can cross blank lines, so a blank separator alone is insufficient proof. Overlapping parser offsets discovered by the mutation matrix are merged into a non-incremental range, preventing the previous invalid-slice panic around edited definitions.

The projection keeps the existing word-edit fast path. For other certified edits, it checks the source predecessor revision and unchanged prefix/suffix, parses only the fragment, replaces its decorations/inline pairs/rich metadata, and rebases retained suffix metadata. Identity mapping pieces can split at the window boundary; non-identity replacements may not cross it. Unchanged highlight lines remain shared. All source/display offsets remain UTF-8 byte offsets. A disclosure change outside the edited window takes the general path.

Plain paragraph invalidation excludes an unchanged terminating newline, allowing the existing layout index to rebase that physical unit. Heading/table/image changes retain broader invalidation so changes to layout unit kinds are rediscovered. The widget, native Iced buffer, focus, IME preedit and document-wide selection are retained. Undo/redo applies the same parsing path once and restores its original source endpoints.

## Interaction results

The same mixed 10k/50k fixtures contain 910,450 / 4,574,450 source bytes. Each version runs in a fresh test process with the same 720×800 viewport and software renderer, serially across the two sizes. Compilation is excluded from the timed command, and no other repository builds/tests run alongside the final profiles. Ordinary interactions have 30 samples; distant jumps have 10. Totals include software rasterization, in milliseconds, p50 / p95.

| Interaction | 10k before | 10k after | 50k before | 50k after |
| --- | ---: | ---: | ---: | ---: |
| Typing / middle | 7.95 / 8.51 | 3.47 / 3.84 | 44.49 / 48.86 | 14.32 / 15.52 |
| Typing / end | 8.10 / 8.82 | 3.48 / 3.80 | 45.19 / 50.74 | 14.95 / 17.22 |
| Local syntax edit | 20.93 / 21.95 | 4.57 / 4.88 | 119.51 / 129.52 | 25.02 / 29.79 |
| Disclosure | 3.16 / 3.31 | 3.05 / 3.34 | 14.59 / 16.58 | 14.19 / 14.79 |
| Half-document selection | 1.61 / 1.92 | 1.63 / 1.82 | 3.52 / 4.04 | 3.91 / 4.07 |
| Keyboard selection | 1.52 / 5.71 | 1.51 / 5.30 | 3.62 / 26.69 | 3.52 / 31.18 |
| Drag selection | 1.68 / 3.80 | 1.75 / 3.77 | 4.46 / 18.65 | 4.26 / 17.13 |
| Scrolling | 1.38 / 1.75 | 1.38 / 1.78 | 1.25 / 1.66 | 1.28 / 1.68 |

At 50k, middle typing is about 68% faster and local syntax edits about 79% faster. The harness retains the historical label `typing/syntax-fallback` for repeated underscore edits; eligible edits in this series now use block parsing. This does not imply that all structural Markdown edits are incremental. The first insertion into trailing blank space still takes the full path: the 50k end-typing maximum remains about 113 ms despite its improved median.

One widget IME preedit/commit and history cycle runs per size, asserting exact source and selection. These are single observations, not distributions:

| Action | 10k before → after | 50k before → after |
| --- | ---: | ---: |
| Unicode commit | 21.48 → 5.68 ms | 130.06 → 25.30 ms |
| Undo commit | 25.46 → 3.59 ms | 158.71 → 20.75 ms |
| Redo commit | 23.77 → 3.60 ms | 146.41 → 20.36 ms |

Load is essentially unchanged: 148 → 152 ms at 10k, 660 → 672 ms at 50k. Initial parsing still covers the full document. The complete two-size process reports about 945 MiB before / 875 MiB after peak RSS, including allocator retention and edited-document caches. These are not isolated per-window memory measurements. No large cold-load or memory improvement is claimed here.

An intermediate version invalidated the whole parsed window including its final newline. At 50k that still cost 11.97 ms of layout for syntax edits. Narrowing only plain paragraph invalidation brings final syntax-edit layout to 0.28 ms, with topology/structural fallback retained. The exploratory baseline overlapped early compilation and is excluded from the comparison; its raw log is retained. Host load, allocator history and software rendering still vary, so these observations are not native latency guarantees.

## Source-only diagnostic

The new release benchmark performs 100 Unicode/delimiter insertions inside a paragraph and asserts that every edit returns a local window. Its byte counters measure input to top-level boundary discovery, not native text snapshots, projection copying or the additional local rendering passes.

| Blocks | 100 edits | Boundary input | Equivalent full input |
| --- | ---: | ---: | ---: |
| 10k | 4.49 ms | 37,300 bytes | 91,065,200 bytes |
| 50k | 9.71 ms | 37,400 bytes | 457,465,200 bytes |

At 50k this avoids more than 99.99% of boundary-parser input for this case. Runtime still grows with suffix offset rebasing and contiguous source replacement. Final ranges, headings and statistics are compared with a fresh complete document. Benchmarks run through `mise run bench`; these component times exclude native actions, projection and rendering and must not be confused with the interaction totals.

## Validation and remaining work

`mise run check` passes formatting, Clippy with warnings denied and 89 regression tests; six profiling/benchmark tests are ignored by default. All four release benchmarks and the complete 10k/50k profile pass, as does `mise run release`.

The shared fixture/mutation matrix checks insertion, deletion and replacement at Unicode boundaries in LF/CRLF documents, including headings, closing hashes, Setext, tables, images, lists, blockquotes, fences, HTML, references, footnotes, definition lists, math, metadata and BOMs. Model ranges, heading data, statistics and dependency flags match a fresh parse. Tables with and without leading pipes, including zero-width decorations at EOF, are covered. Accepted block projections match full text, highlights, rich data, parsed reusable metadata, disclosure, line starts and every UTF-8 source/display mapping in both affinities. Repeated splits/replacements and undo/redo exercise changing topology and fallback transitions.

Application coverage verifies Unicode composition payloads, shared unaffected styles, native cursor restoration, reverse selection and history through local edits. The virtual-layout differential now includes Unicode changes, paragraph splitting and table formatting, comparing its unit index and exact resident caret geometry with fresh layout. Existing cross-block selection/dragging, clipboard, disclosure anchors, wrap affinity and transient IME tests remain intact. The full profile sends real widget input-method events and verifies that preedit stays transient and commit inserts once.

This remains a conservative incremental parser, not a general incremental CommonMark engine. Global references/footnotes, container neighborhoods, cross-block replacement and uncertain edits can still reparse the document. Native buffer snapshots, metadata cloning/rebasing, contiguous source changes, disclosure projection and outline construction still have linear costs. Local Unicode/syntax updates at 50k still exceed a 16 ms frame budget in this software path. Native OS input, actual CJK composition, GPU presentation, accessibility, file I/O and image decoding are outside this harness. Existing user windows and unsaved documents were preserved.

## Reproduce and evidence

```sh
mise run check
mise run bench
mise run profile
STILL_PROFILE_BLOCKS=50000 STILL_PROFILE_SAMPLES=30 mise run profile
```

For process metrics without compilation, first build with `mise exec -- cargo test --locked --release --lib --no-run`, then run the emitted executable under `/usr/bin/time -l` with `large_document_interactions --ignored --nocapture --test-threads=1`. To reproduce the baseline, archive `bbbeb0f` into a temporary directory, build using mise from the trusted repository with `--manifest-path` pointing there and a shared `CARGO_TARGET_DIR`, and run its executable before rebuilding the current version. Both builds can emit the same executable filename. The interaction harness and fixtures are unchanged; the source-only benchmark is new.

- [Isolated baseline](performance/incremental-parsing/before-interactions.txt).
- [Final interaction profile](performance/incremental-parsing/after-interactions.txt).
- [Prior after run](performance/incremental-parsing/second-interactions.txt): before the final EOF metadata ownership regression; 50k syntax median 26.73 ms.
- [Intermediate layout invalidation](performance/incremental-parsing/intermediate-interactions.txt).
- [Exploratory baseline, excluded](performance/incremental-parsing/exploratory-before-interactions.txt).
- [Release model/layout benchmarks and parser byte counters](performance/incremental-parsing/model-layout.txt).
