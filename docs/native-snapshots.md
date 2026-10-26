# Native buffer snapshots

Measured on 2026-10-22 with mise's Rust 1.95.0, locked Iced 0.14 / cosmic-text 0.15, release optimization, macOS 26.7.1 on Apple silicon (arm64), and bundled Newsreader fonts. The baseline is `10507ff`, after [incremental block parsing](incremental-parsing.md).

The subsequent [source/projection update report](source-projection-updates.md) addresses text and metadata copying after native snapshotting.

## Snapshot ownership

Previously, each native edit called `Content::text()`. Iced clones each physical line and concatenates a new document-sized String. Navigation already borrowed the existing projection, but typing and IME commits still paid for those allocations and copies.

`editor/native_buffer.rs` now owns the standard Iced content alongside a reusable String and a physical line-start index. Widgets borrow immutable content; every mutation goes through this owner. After Iced performs an edit, the wrapper reads the span between the old selection endpoints, plus one neighboring line on either side, adjusted by the native line-count change. It splices that window into the retained snapshot and rebases subsequent offsets. The native backend still decides insertion, grapheme deletion, line joining and indentation. Invalid line-count bounds fall back to a complete native read.

The same index converts native cursor/anchor positions to byte offsets directly and locates source/display selections by binary search. Navigation copies no native text. The index follows the actual native lines after edits, rather than reparsing concatenated mixed line endings. A CR followed by LF from different physical lines must not accidentally become one indexed CRLF line.

Source, history and disclosure synchronization update this same snapshot through native actions. Distant disclosure still applies proven regions from the end. Minimal byte diffs that split a CRLF/LFCR pair expand to complete native endings, retaining the unchanged bytes in the replacement. This fixes a Source-to-Live synchronization edge case exposed by the new exact native-text assertions. The native buffer, widget focus, preedit state, source selections, wrap affinity and scroll anchoring remain continuous.

This trades repeated temporary snapshots for one retained text copy and line index. String capacity growth can still copy the mirror; length changes still shift its suffix and rebase line offsets. The optimization removes whole-document reconstruction on ordinary edits, not all linear work.

## Interaction results

The unchanged mixed fixtures contain 10k/50k blocks and 910,450/4,574,450 source bytes. Each version runs in a fresh process with both sizes, a persistent widget tree, and a 720×800 software-rendered viewport. Compilation and other repository builds/tests are excluded from the profiles. Ordinary interactions have 30 samples; distant jumps have 10. Totals below include event dispatch, App update, layout, drawing and software rasterization, in milliseconds, p50 / p95.

| Interaction | 10k before | 10k after | 50k before | 50k after |
| --- | ---: | ---: | ---: | ---: |
| Typing / middle | 3.29 / 3.83 | 2.57 / 2.92 | 14.48 / 16.82 | 9.36 / 9.50 |
| Typing / end | 3.34 / 3.99 | 2.43 / 2.70 | 15.42 / 18.52 | 9.27 / 10.73 |
| Local syntax edit | 4.58 / 5.30 | 3.56 / 3.90 | 26.98 / 31.45 | 23.67 / 28.25 |
| Disclosure | 3.11 / 3.51 | 2.72 / 3.30 | 16.28 / 17.25 | 13.01 / 13.58 |
| Half-document selection | 1.64 / 1.85 | 1.30 / 1.40 | 3.50 / 4.01 | 2.17 / 2.26 |
| Keyboard selection | 1.52 / 5.46 | 1.15 / 5.19 | 3.49 / 26.51 | 2.04 / 26.90 |
| Drag selection | 1.84 / 3.94 | 1.45 / 3.50 | 3.95 / 15.35 | 2.89 / 17.36 |
| Scrolling | 1.39 / 1.65 | 1.39 / 1.73 | 1.26 / 1.68 | 1.28 / 1.67 |

At 50k, middle typing is about 35% faster. Its median App update falls from 13.33 to 8.25 ms; layout remains about 0.25 ms. Both snapshot reconstruction and repeated cursor scans are removed. Selection medians improve, while disclosure and geometry changes still dominate keyboard/drag tails. The historical `typing/syntax-fallback` label now includes eligible block parsing; this series does not represent every Markdown edit.

One widget composition/history cycle runs per size, asserting transient preedit, a single Unicode commit, and exact undo/redo source and selection. These are single observations, not distributions:

| Action | 10k before → after | 50k before → after |
| --- | ---: | ---: |
| Unicode commit | 8.11 → 5.24 ms | 25.63 → 24.09 ms |
| Undo commit | 3.77 → 4.03 ms | 21.58 → 24.20 ms |
| Redo commit | 4.11 → 3.51 ms | 20.65 → 23.53 ms |

No history speedup is claimed. These operations still copy/rebase source and projection metadata; the observed 50k undo/redo times increase. Syntax edits and Unicode commits remain above a 16 ms frame budget. Inserting into trailing blank space still takes the full parser/layout path: 50k end-typing maximum is 108 ms, versus 121 ms before.

Load is 150 → 186 ms at 10k and 668 → 671 ms at 50k, each a single observation. The retained mirror adds storage, so no cold-load improvement is claimed. Peak RSS across the full two-size process is about 877 → 681 MiB. This includes allocator retention, caches and host reclamation, not isolated window memory or allocated-byte counts. The baseline also records more page faults. These peaks do not establish a native memory budget or a guaranteed 196 MiB saving.

## Isolated snapshot diagnostic

The new release benchmark compares 100 native Unicode insertions with a full snapshot after each edit against the same actions using the retained mirror. Its plain paragraph fixtures are 400,000/2,000,000 bytes, separate from the mixed interaction fixtures. Initialization and final correctness reads are outside the timer. The following run is serial and isolated from other tests:

| Blocks | Full snapshots | Retained snapshot | Full text bytes | Local window bytes |
| --- | ---: | ---: | ---: | ---: |
| 10k | 53.26 ms | 0.582 ms | 40,015,150 | 19,250 |
| 50k | 280.63 ms | 2.986 ms | 200,015,150 | 19,250 |

The counter records native text assembled for snapshot reads, including endings. It excludes initial mirror construction, internal line clones, String suffix moves/reallocation, native mutation, and other application snapshots. At 50k, this avoids about 99.99% of snapshot read bytes in this case. It is not an allocation count or a prediction of full application speedup. `mise run bench` also runs all five release benchmarks, concurrently by default; its raw results are retained separately.

## Validation and remaining limits

`mise run check` passes formatting, Clippy with warnings denied, and 94 regression tests; seven benchmarks/profiles are ignored by default. All five release benchmarks, the complete 10k/50k interaction profile, and `mise run release` pass. The macOS bundle is rebuilt and ad-hoc signed without opening or restarting existing windows.

Differential tests compare snapshots, native text, physical line starts, cursor, selection and line endings after every edit kind at Unicode boundaries, with forward/reverse selections, empty documents, CR/LF/CRLF/LFCR, RTL, combining characters and joined emoji. Repeated motions, word/line selection, indentation, paste and line joins cross deferred-load batch boundaries. Exhaustive synchronization splices cover endpoints inside paired endings. Application tests assert snapshot/native/projection agreement after messages, including Source/Live switching, CRLF reverse replacement, disclosure, table/image editing and undo/redo. Full native reads used as correctness oracles in benchmarks/profiles remain outside the timed edit path. Existing rich-widget tests cover scroll anchors, wrap affinity, cross-block dragging and transient IME composition.

Large selected spans and exceptionally long physical lines can still require large native reads. The mirror and its index remain contiguous; source/projection copying, metadata rebasing, conservative parser fallbacks and outline construction retain linear costs. Native shaping caches can accumulate, and the stock Source widget still has metric-related shaping costs. The 16 MiB document guard and patch-based undo remain in place.

These are CPU/software measurements, excluding native OS input delivery, GPU presentation, accessibility, file I/O and image decoding. Actual CJK IME integration still needs native manual testing. Existing user windows and unsaved documents were preserved.

## Reproduce and evidence

```sh
mise run check
mise run bench
mise run profile
mise run release
mise exec -- cargo test --locked --release --lib --no-run
```

Run the emitted library test executable directly, replacing `<hash>` with its actual filename:

```sh
/usr/bin/time -l target/release/deps/still-<hash> large_document_interactions --ignored --nocapture --test-threads=1
target/release/deps/still-<hash> performance_native_snapshots --ignored --nocapture --test-threads=1
```

The baseline was profiled before edits. To reproduce it, archive `10507ff` to a temporary directory and build through mise from the trusted repository with `--manifest-path` pointing to the archive. A shared `CARGO_TARGET_DIR` reuses dependencies; run the baseline executable before rebuilding the current version because the filename can be identical. The interaction sequence is unchanged; the current harness adds complete native/snapshot equality checks after timing. The load-component diagnostic now includes the mirror and index. The older 2k continuous-edit benchmark keeps correctness reads outside its edit timer as well.

- [Baseline interactions and process metrics](performance/native-snapshots/before-interactions.txt).
- [Final interactions and process metrics](performance/native-snapshots/after-interactions.txt).
- [Serial native snapshot diagnostic](performance/native-snapshots/snapshot-component.txt).
- [All release benchmarks](performance/native-snapshots/model-layout.txt).
