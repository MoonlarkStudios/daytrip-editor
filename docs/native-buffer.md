# Native buffer initialization

Measured on 2026-10-22 with mise's Rust 1.95.0, Cargo.lock's Iced 0.14 / cosmic-text 0.15, release optimization, macOS 26.7.1 / arm64, and the bundled Newsreader fonts. The baseline is commit `ebdcded`, which already includes [viewport virtualization](virtualization.md).

The subsequent [incremental block parsing report](incremental-parsing.md) addresses parsing during edits on top of this implementation.

## Implementation

`Content::with_text` creates an unbounded cosmic-text buffer and shapes all its lines. That work duplicates the live widget's separate rich layout. Initial native glyph storage dominated memory even after rich layout became virtual.

`editor/native_buffer.rs` instead creates empty Iced content and populates it with ordinary native paste actions at the end. A batch ends after 128 physical lines or 64 KiB, always at a line boundary; a single long line can exceed that byte threshold. Batching matters because cosmic-text inserts pasted lines individually into a Vec. One document-sized paste would shift a growing tail repeatedly and become quadratic. Appending bounded batches keeps that tail small. Line boundaries come from the exact cosmic-text version re-exported by Iced, including CRLF, CR, LF and LFCR pairs. The cursor returns to the document start after population.

The result is still the same Iced `Content`, with the same native Unicode motions and mutation actions. No dependency, renderer, buffer implementation, selection mapping, history mechanism, or widget identity changes. Native shapes are produced when an action needs them. The live widget continues to own exact viewport geometry, source anchors, selection drawing and IME positioning. Existing sparse disclosure splices remain intact.

## Cold load and memory

Each table entry uses a fresh test process, one document size per process, with compilation excluded from `/usr/bin/time -l`. The baseline archive and current build use the same locked dependencies, toolchain, mixed Markdown fixtures, 720×800 viewport and software renderer. Sizes are 910,450 / 4,574,450 source bytes. These are single cold observations, not percentiles. Fonts are loaded before the harness load timer.

| Measurement | 10k before | 10k after | 50k before | 50k after |
| --- | ---: | ---: | ---: | ---: |
| Source/model/native load | 650.87 ms | 150.33 ms | 3136.83 ms | 695.31 ms |
| First rich layout | 8.86 ms | 8.84 ms | 22.48 ms | 19.35 ms |
| First frame with software rasterization | 11.15 ms | 11.02 ms | 24.48 ms | 21.40 ms |
| Load + first frame | 662.02 ms | 161.34 ms | 3161.30 ms | 716.71 ms |
| Peak resident memory | 304.55 MiB | 117.33 MiB | 1275.86 MiB | 437.80 MiB |
| Resident rich fragments / visual rows | 44 / 52 | 44 / 52 | 44 / 52 | 44 / 52 |

At 50k, load to first frame is about 77% shorter and cold peak RSS about 66% lower. The isolated load diagnostic records native initialization falling from 2486.42 ms to 87.61 ms (about 28× faster). Document construction remains approximately 565–571 ms and projection 66–69 ms in that diagnostic, making top-level parsing the main remaining load cost.

The first exploratory after runs took 550 ms / 2710 ms to load, with about 119 / 405 MiB peak RSS. They had more page faults and substantially more elapsed time than CPU time; the final runs recorded 150 / 695 ms, consistent with the full interaction series' 149 / 674 ms loads. Both exploratory logs are retained. This is evidence of host/cache/scheduling variation, not a latency guarantee. Peak RSS also varies with macOS reclamation and allocator history; instantaneous `ps` snapshots must not be subtracted as allocation counts.

## Editing baseline

The complete serial profile passes at both sizes. Ordinary interactions use 30 samples each; distant jumps use 10. Totals below include software rasterization, in milliseconds, p50 / p95.

| Interaction | 10k | 50k |
| --- | ---: | ---: |
| Typing / middle | 7.81 / 8.26 | 41.44 / 44.17 |
| Contextual disclosure | 2.97 / 3.71 | 14.00 / 15.13 |
| Half-document selection | 1.64 / 1.85 | 4.03 / 4.68 |
| Keyboard selection | 1.47 / 5.72 | 3.59 / 26.41 |
| Drag selection | 1.69 / 3.90 | 4.41 / 17.14 |
| Scrolling | 1.34 / 1.81 | 1.39 / 1.86 |
| Structural syntax edit | 21.12 / 22.52 | 123.94 / 138.32 |

These remain within the broad range of prior runs; this change targets loading and initial native allocations. It does not improve the whole-document work on each edit. For example, the prior 50k middle-typing median was 37.84 ms, with an earlier run near 46 ms. No warm-typing speedup is claimed.

The profile additionally sends real widget input-method events at depth: Opened → Preedit → Commit. Assertions verify preedit changes neither source nor selection, committed Japanese/combining/emoji text inserts exactly once, and undo/redo restore exact source and selection. These are one observation per size, not latency distributions. At 50k the Unicode commit takes 131 ms, undo 168 ms and redo 163 ms, through structural parsing. Composition is correct in this harness but those model costs still need improvement. The complete two-size process peaks at about 680 MiB RSS; it includes allocator retention, edited-document caches and both fixtures, so it is distinct from the fresh cold table.

## Validation and limits

Differential tests compare deferred content with Iced's original constructor: exact text, line endings, line count, cursor, copied selection, Unicode/RTL motions, word selection, paste, Enter, delete and backspace across batch boundaries. Empty documents, trailing endings and a large single line are covered. An application regression replaces a reverse selection spanning headings, tables and images across several batches, toggles Source mode and checks undo/redo source and endpoints. Existing rich-widget tests now use the real deferred constructor, including clipboard, drag selection, disclosure scroll anchors, wrap affinity and transient IME preedit.

`mise run check`, `mise run bench`, the full 10k/50k profile, and `mise run release` pass. Timings are observations; ordinary tests assert behavior, not machine-dependent deadlines.

Text storage is still document-wide and native text snapshots still allocate per line. This is deferred initialization, not a hard native shaping-cache budget. Native shapes can accumulate on visited lines. The stock Source widget can still shape extensively when Iced changes metrics before setting finite bounds; the cold table measures opening in Live mode. Large selection deletion and multiline paste still use cosmic-text's existing algorithms. Structural parsing, outline UI, and very long physical lines remain scaling limits.

These measurements exclude native window chrome, OS input delivery, GPU presentation, file I/O, network/image decoding and accessibility. Real CJK composition and platform integration require native manual testing. Existing user windows and unsaved documents were left untouched.

## Reproduce and raw evidence

```sh
mise run check
mise run bench
mise run profile
mise exec -- cargo test --locked --release --lib --no-run
```

Run the emitted library test executable directly under `/usr/bin/time -l`, one fresh process per size:

```sh
STILL_PROFILE_BLOCKS=10000 STILL_PROFILE_COLD_ONLY=1 /usr/bin/time -l target/release/deps/still-<hash> large_document_interactions --ignored --nocapture --test-threads=1
STILL_PROFILE_BLOCKS=50000 STILL_PROFILE_COLD_ONLY=1 /usr/bin/time -l target/release/deps/still-<hash> large_document_interactions --ignored --nocapture --test-threads=1
/usr/bin/time -l target/release/deps/still-<hash> large_document_load_components --ignored --nocapture --test-threads=1
```

For the baseline, archive `ebdcded` into a temporary directory. Build its library tests using mise from the trusted repository, passing `--manifest-path` to that archive and the same `CARGO_TARGET_DIR` to reuse dependencies. Run each baseline measurement before rebuilding the current version; both versions may emit the same test executable filename. Cold-load and component diagnostics were already present in that commit. The current full interaction harness adds composition/history assertions after the existing interaction series.

- Fresh cold runs: [before 10k](performance/native-buffer/before-cold-10k.txt), [after 10k](performance/native-buffer/after-cold-10k.txt), [before 50k](performance/native-buffer/before-cold-50k.txt), [after 50k](performance/native-buffer/after-cold-50k.txt).
- [Native/model breakdown before](performance/native-buffer/before-components.txt) and [after](performance/native-buffer/after-components.txt).
- [Complete interactions, composition and history](performance/native-buffer/after-interactions.txt).
- Exploratory cold runs: [10k](performance/native-buffer/exploratory-cold-10k.txt), [50k](performance/native-buffer/exploratory-cold-50k.txt).
- [Release model/layout benchmarks](performance/native-buffer/model-layout.txt).
