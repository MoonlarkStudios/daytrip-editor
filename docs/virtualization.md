# Viewport virtualization

Measured on 2026-10-22 with mise's Rust 1.95.0, Iced 0.14, release optimization, macOS/Apple silicon, and the bundled Newsreader fonts. The baseline is commit `63e4ae7`; its source organization has the same behavior as the [previous performance baseline](performance.md).

## Behavior and boundaries

Large documents keep a lightweight index of physical paragraphs and table rows, with a Fenwick tree of estimated/measured heights. Only the viewport, one viewport of overscan on each side, and adjacent navigation units retain shaped paragraphs, glyph stops, fragments, and decorations. Documents with at most 256 units use exact full layout. Height hints survive eviction; shaped text requires an exact cache-key match. Local projection changes rebase index offsets without hashing the entire document again.

All editing still happens in Live mode with one document-wide source selection. Distant navigation measures its destination directly. A remote selection anchor does not materialize intervening blocks. Hits, visual motion, caret placement, selection rectangles, and table cells use exact resident geometry. Wheel, scrollbar, and drag scrolling prepare their new window before hit testing or drawing. Source anchors compensate for height refinement and disclosure/metric changes; upstream/downstream wrap intent survives eviction. Estimated scrollbar extent can refine while visiting previously unmeasured text.

Moving disclosure between distant headings also exposed a native-buffer defect: one envelope splice deleted and reinserted the unchanged text between them. A sampled baseline spent its time in cosmic-text's line deletion/memmove. Proven projection regions now produce separate local splices, applied from the end. This includes multiline Setext disclosure and preserves native selection and source history.

## Cold layout and memory

These are **one fresh test process per size and version**, without compilation in the timed command. The baseline archive uses the identical profiling harness and memory probes. Source fixtures, 720×800 viewport, fonts, and rendering path match. Cold figures are single observations, not interaction percentiles.

| Measurement | 10k before | 10k after | 50k before | 50k after |
| --- | ---: | ---: | ---: | ---: |
| Source/model/native load | 658 ms | 636 ms | 3141 ms | 3070 ms |
| Initial rich layout | 337 ms | 7.98 ms | 1603 ms | 19.93 ms |
| First frame including software rasterization | 347 ms | 10.24 ms | 1607 ms | 21.74 ms |
| Load + first frame | 1005 ms | 646 ms | 4748 ms | 3091 ms |
| Peak resident memory | 500 MiB | 306 MiB | 2152 MiB | 1375 MiB |
| Initial retained fragments / visual rows | 26001 / 30001 | 44 / 52 | 130001 / 150001 | 44 / 52 |

At 50k blocks, rich layout is about 99% cheaper and fresh-process peak RSS is about 36% lower (2.10 → 1.34 GiB). Whole load-to-first-frame time improves about 35%, because native buffer initialization remains expensive. After the full interaction series, the resident window contained 65 fragments and 77 rows at both sizes.

Peak RSS comes from `/usr/bin/time -l` around the test executable. The raw logs also contain instantaneous test-process RSS from `ps`, in KiB. Snapshots fluctuate with allocation, reclamation, and macOS memory compression; they are not reliable allocation deltas. The cold table uses peak RSS rather than subtracting snapshots. This measures the harness, including its native buffer, source/projection, and software renderer; it excludes a native window, outline UI, GPU presentation, file I/O, network image loading, and decoded-image memory.

## Warm interactions

Final totals in milliseconds, **p50 / p95**, including software rasterization. There are 30 samples per ordinary interaction and 10 distinct distant jumps, run serially. The mixed fixtures contain 10k/50k Markdown blocks and 910,450/4,574,450 source bytes.

| Interaction | 10k | 50k |
| --- | ---: | ---: |
| Typing / middle | 7.46 / 8.06 | 37.84 / 39.60 |
| Contextual disclosure | 2.86 / 2.99 | 15.01 / 15.73 |
| Half-document selection | 1.59 / 1.68 | 3.97 / 4.34 |
| Keyboard selection | 1.40 / 5.07 | 3.76 / 26.85 |
| Drag selection | 1.69 / 3.30 | 4.07 / 16.84 |
| Scrolling | 1.36 / 1.72 | 1.36 / 1.79 |
| Structural syntax edit | 19.63 / 20.27 | 113.54 / 122.09 |
| Distant source jump (10 samples) | 3.92 / 4.82 | 14.25 / 15.13 |

The extended baseline's 10k distant jumps took about 1958 ms median, mostly native synchronization. Its 50k distant-jump series was interrupted after sampling the quadratic deletion; both cold baselines and the final interaction runs completed. Do not treat that interruption as a current test failure or assign a 50k jump percentile to it.

An intermediate implementation rebuilt and hashed the entire height index for each edit/disclosure. At 50k this spent about 12–14 ms in layout per update. Index rebasing reduces final middle-typing layout to 0.26 ms median and disclosure layout to 0.28 ms. Ordinary scrolling prepares new geometry in the event phase; its measured p95 event cost is 0.20 ms at 50k. Table/structural transitions can still rebuild the lightweight index.

Whole-document updates remain the main typing cost. The previous baseline's 50k middle-typing total was about 41 ms; the final run records about 38 ms. An earlier post-index run recorded about 46 ms, with the variation concentrated in the model/native update phase (44.41 versus 36.73 ms median); rich layout stayed below 0.4 ms in both. These runs include different preceding navigation, host load, and allocator histories. The strong results here are lower layout and memory costs; whole-document typing is still proportional to document size. Raw logs retain phase distributions and maxima; phase percentiles must not be added to derive total percentiles.

## Validation and remaining limits

`mise run check` passes formatting, Clippy with warnings denied, and 80 regression tests (five profiling tests remain ignored by default). The three release model/layout benchmarks and the full 10k/50k profile pass.

New tests compare resident caret, hit testing, selection geometry, and page movement with full layout after distant eviction. They check height sums/inverse queries, 50k-block residency bounds, a 2000-row table, index topology and exact geometry through edits/disclosure, reverse native selection/history through distant Setext disclosure, source scroll anchors during disclosure and resizing, drag anchors across evictions, and wrap affinity when navigating back to an evicted caret or jumping into a new disclosure context, and viewport reset on document replacement. Existing clipboard, undo, rich-block, IME commit, file-safety, and theme regressions remain intact.

The native Iced `Content` is still a full-document buffer; its current initialization shapes the full text and dominates remaining memory/load cost. Document parsing/projection and structural index rebuilds remain proportional to document size. A single very long physical paragraph or table row is still one shaping unit. Native input-to-photon latency, real IME behavior, accessibility, GPU memory, and platform behavior require separate native testing.

## Reproduce and evidence

```sh
mise run check
mise run bench
mise run profile
STILL_PROFILE_BLOCKS=50000 STILL_PROFILE_SAMPLES=30 mise run profile
STILL_PROFILE_BLOCKS=10000 STILL_PROFILE_COLD_ONLY=1 mise run profile
STILL_PROFILE_BLOCKS=50000 STILL_PROFILE_COLD_ONLY=1 mise run profile
```

For peak memory independent of compilation, build the release library test with `mise exec -- cargo test --locked --release --lib --no-run`, then run the resulting executable directly under `/usr/bin/time -l` with `large_document_interactions --ignored --nocapture --test-threads=1` and the cold-only environment above. Run each size in its own process. To reproduce the baseline, archive `63e4ae7` into a temporary directory and copy the current `src/app/performance.rs` harness into it before building with the same toolchain and lockfile.

- Fresh cold runs: [before 10k](performance/virtualization/before-cold-10k.txt), [after 10k](performance/virtualization/after-cold-10k.txt), [before 50k](performance/virtualization/before-cold-50k.txt), [after 50k](performance/virtualization/after-cold-50k.txt).
- [Extended baseline interactions](performance/virtualization/before-interactions.txt): complete 10k series and 50k cold frame; intentionally interrupted during distant navigation (SIGINT footer).
- [Intermediate implementation](performance/virtualization/intermediate-interactions.txt): exposes the initial index-rebuild cost.
- [First index-rebased series](performance/virtualization/second-interactions.txt): records the earlier 46 ms typing observation.
- [Final interactions](performance/virtualization/after-interactions.txt): 30 samples per interaction at both sizes; 10 distant jumps each.
- [Model/layout benchmarks](performance/virtualization/model-layout.txt).
