# Incremental offline imports

Measured 2026-10-23 against clean baseline **229d89d**, Apple M2 Max / 32 GiB, macOS 26.7.1, mise Rust 1.95.0, Iced 0.14, release/thin LTO/codegen-units=1. [Environment](performance/offline-imports/environment.txt) and [measured source hashes](performance/offline-imports/source-sha256.txt) are retained. No dependency or CRDT wire-format change is involved.

## What changed

Admission still validates and stages data before mutating the active Yrs Doc. It first validates the **entire original envelope**, including already-known operations. Yrs `diff_updates_v1` then removes known insertions using the active integrated state vector; delete sets and unknown/pending structures remain. The same filtered operations are applied to the isolated candidate and, after validation, the existing active Doc. This avoids decoding the full incoming replica separately for staging and publication. A truly empty filtered update avoids staging; a delete-only update cannot take that shortcut. Full encoded checkpoints still preserve pending operations. Candidate encoded size supplies exact retained-byte accounting; all existing text, encoded-state, structure, and edit limits remain. Nonempty ingress retains the conservative predecessor-plus-original-envelope size guard. An empty filtered update needs no second full-state allocation.

The source cache groups sorted original-state edits by affected block. It validates every island before changing any cache, parses each affected block once, then applies source splices and cached blocks from right to left. Multiple edits in one block share a parse. It records a list of certified old/new windows with one predecessor revision. These are derived cache proofs; CRDT edits remain granular and authoritative.

Projection preflights all windows, metadata containment, unchanged gaps, physical lines, and disclosure. It parses the small fragments, retains unaffected rich payloads/highlights, and constructs **one immutable successor map** by copying each unchanged mapping gap once. Old mapping, line and highlight snapshots remain available to layout/anchoring. Strings and metadata receive descending splices only after every fallible check passes. One source/highlight revision describes the complete batch, with sorted sparse display regions passed to native-buffer synchronization and rich layout. This avoids rebuilding the full projection or replacing the text between distant edits. Source and display coordinates stay distinct, including table cells and images.

Incremental admission to this cache path is bounded to **32 independent windows**, each at most **64 KiB**. Larger batches, global Markdown dependencies, uncertain boundaries/containers, or disclosure changes outside the windows use the full scanner once and general projection. Nothing is truncated or partially accepted. The cap bounds repeated metadata validation/rebasing; it is a performance fallback, not an operation limit.

The active Doc, stable positions, origin-scoped undo managers and widget tree are retained. Receiving replicas emit no Markdown repair transactions. Selection direction, drag/viewport anchors, composition targets, wrap affinity, and exactly-once IME commit keep their existing semantics. No async task replaces the live Doc and no extra replicated edits are introduced.

## Comparable 10k/50k editor measurements

The fresh before/after runs use the existing 910,450 / 4,574,450-byte fixtures, Newsreader fonts, 720×800 viewport, persistent native widget tree and software rasterizer. Thirty compatible full replica states each add eight disjoint Unicode insertions. Peer edits/export happen outside the timer. The timed path includes admission, source/cache, App projection/native synchronization, layout, draw and software rasterization. It excludes file IO/dialogs, OS event delivery, GPU presentation, decoded images and window chrome. All final measurement processes ran serially with builds completed and no competing builds. Full source/projection test oracles run outside timed paths.

Total p50 / p95 / maximum milliseconds (30 samples):

| Blocks | Before | After |
| --- | ---: | ---: |
| 10,000 | 28.421 / 41.296 / 46.189 | 9.241 / 10.951 / 17.464 |
| 50,000 | 154.509 / 193.918 / 195.017 | 38.423 / 44.511 / 86.339 |

Median total fell **67.5% at 10k** and **75.1% at 50k**. Independent phase medians follow; they cannot be summed to reconstruct a total percentile.

| Phase, ms | 10k before → after | 50k before → after |
| --- | ---: | ---: |
| crdt | 6.946 → 5.602 | 35.628 → 26.324 |
| source | 4.937 → 0.204 | 26.939 → 0.692 |
| adapters | 13.127 → 2.221 | 77.596 → 10.282 |
| layout | 2.454 → 0.065 | 13.670 → 0.182 |
| raster | 0.991 → 1.007 | 1.017 → 0.938 |

The regular import still costs about 38 ms at 50k. Staged admission remains the largest portion; this is an offline-import optimization, not a real-time frame-budget guarantee.

## Larger and structural batches

A separate **after-only** profile uses ten batches per shape and size. Each sample compares exact source, fresh projection text/rich metadata/highlights, and the native snapshot after timing. The 32-window case certified 10/10 batches, the 128-edit case deliberately used the full fallback 10/10 times, and the structural case certified 10/10. These cases have no matched pre-change measurements and are not used to claim a speedup.

| Shape | 10k p50 / p95 / max, ms | 50k p50 / p95 / max, ms | First 50k import, ms |
| --- | ---: | ---: | ---: |
| sparse-32 | 22.165 / 71.243 / 71.243 | 116.378 / 422.501 / 422.501 | 422.501 |
| fallback-128 | 48.515 / 266.891 / 266.891 | 182.990 / 1011.229 / 1011.229 | 1011.229 |
| structural-8 | 25.531 / 39.308 / 39.308 | 64.382 / 125.402 / 125.402 | 125.402 |

**Large first imports still stall.** In the 128-edit 50k case, the first import exceeded one second, with roughly 846 ms in CRDT admission. Inspection of the pinned Yrs implementation shows UTF-16 traversal and retained-string copying when a large existing item is repeatedly split; this is the likely cause of the large first-import cost, rather than full parsing alone. The cache optimization does not eliminate it. Later large batches also retain whole-state staging and the conservative projection fallback. Worker admission and faster intra-item splitting remain concrete future work; replacing the live Doc would need to preserve its history and anchors.

An earlier extended run is retained separately in `batch-shapes-initial.txt`: its 50k medians were 71.277 / 174.965 / 64.703 ms for the three shapes, versus the final run above. The rerun added first-sample reporting outside the timers. These machine/run variations are not pooled or selectively omitted.

## Editing behavior, memory, and retained state

The fresh standard interaction profile covers 30 warm typing/disclosure/selection/scroll samples, with IME/undo/redo and cold initialization as single observations. Median total milliseconds:

| Interaction | 10k before → after | 50k before → after |
| --- | ---: | ---: |
| cold | 25.437 → 11.705 | 23.214 → 15.790 |
| typing/start | 2.005 → 1.934 | 6.403 → 5.128 |
| typing/middle | 1.866 → 1.726 | 5.836 → 4.886 |
| typing/end | 1.965 → 1.556 | 5.096 → 4.666 |
| typing/syntax-fallback | 1.850 → 1.702 | 5.092 → 5.265 |
| disclosure | 3.099 → 2.791 | 14.687 → 14.752 |
| selection/half-document | 1.378 → 1.284 | 2.394 → 2.277 |
| selection/keyboard | 1.256 → 1.179 | 2.511 → 2.193 |
| selection/drag | 1.539 → 1.395 | 2.514 → 2.548 |
| scroll | 1.502 → 1.340 | 1.531 → 1.321 |
| ime/commit | 3.075 → 3.008 | 6.718 → 7.437 |
| undo/ime | 1.714 → 1.794 | 5.312 → 6.121 |
| redo/ime | 1.775 → 1.889 | 4.872 → 4.718 |

Source/App initialization was **196.96 → 184.95 ms** at 10k and **763.97 → 692.52 ms** at 50k (n=1), separate from the first-frame `cold` row. These are fresh-instance observations, not cold-boot distributions.

No typing/scrolling speedup is attributed to this import-only change. Single-observation IME/undo times vary; correctness is covered by native-widget tests, while OS IME delivery still needs manual validation. Full-parser and structural layout costs remain visible in the raw tails.

The subsequent 200 paste/undo pairs (400 rendered operations) measured **1.982 → 1.929 ms** median at 10k and **4.617 → 5.086 ms** at 50k. State growth matched baseline: checkpoints **916,791 → 921,029 B** and **4,580,919 → 4,585,159 B** after churn, with the original visible source restored. No tombstone/history compaction was introduced.

The comparable remote/churn process peak fell **688.1 → 514.3 MiB** (about 25%). After 50k remote batches its process RSS was **701,248 → 481,104 KiB**; after application churn **703,248 → 434,224 KiB**. The separate core-profile peaks were **694.4 → 681.2 MiB**. These include App/source/projection/native buffers, renderer and allocator retention, not isolated CRDT memory. The extended multi-shape process has a different lifetime/workload and is not a matched memory comparison.

All seven release component benchmarks also passed. The distributed model churn still ends with exact original source: 2,000 transactions at 50k took **473.91 ms** (245.73 ms CRDT, 227.99 ms cache), encoded state **4,574,477 → 4,655,078 B**, and model reopen **573.90 ms**. These component numbers exclude native widgets/rendering and are not substitutes for import latency.

## Validation and reproduction

All **133 ordinary application tests** pass, along with seven release component benchmarks, the existing core and remote/churn profiles, and the extended batch-shape profile. New tests cover every disclosure character position in mixed CRLF/Unicode headings, tables, images and paragraphs; immutable predecessor mapping/highlight snapshots; rejection without partial projection mutation; delete-only and malformed-known envelopes; mixed full-parser fallbacks; and real widget reverse selection, preedit/exactly-once commit, undo/redo and viewport anchoring through sparse imports. Existing deterministic three-replica convergence, pending/out-of-order/duplicate delivery, restart, independent human/agent history and Source/Live IME tests remain active. No timing assertions were added.

```sh
mise run check
mise run release
mise exec -- cargo test --locked --release --lib --no-run
# Use the executable emitted above; this checkout emitted:
daytrip_profile_exe=target/release/deps/daytrip-508656e959175f01
/usr/bin/time -l "$daytrip_profile_exe" large_document_collaboration --ignored --nocapture --test-threads=1
/usr/bin/time -l "$daytrip_profile_exe" large_document_interactions --ignored --nocapture --test-threads=1
/usr/bin/time -l "$daytrip_profile_exe" large_offline_import_shapes --ignored --nocapture --test-threads=1
# DAYTRIP_IMPORT_SAMPLES controls the extended profile (default 10).
RUST_TEST_THREADS=1 mise run bench
mise run profile
```

- [Fresh baseline imports](performance/offline-imports/before-remote.txt), [final imports](performance/offline-imports/after-remote.txt).
- [Fresh baseline interactions](performance/offline-imports/before.txt), [final interactions](performance/offline-imports/after.txt), [independent mise profile](performance/offline-imports/mise-profile.txt).
- [Extended shapes and first imports](performance/offline-imports/batch-shapes.txt), [earlier extended run](performance/offline-imports/batch-shapes-initial.txt), [three-sample diagnostic before the single-map optimization](performance/offline-imports/diagnostic.txt).
- [Required checks](performance/offline-imports/check.txt), [release](performance/offline-imports/release.txt), [test build](performance/offline-imports/test-build.txt), [benchmarks](performance/offline-imports/bench.txt), [bundle/signature verification](performance/offline-imports/bundle.txt).

Existing editor windows and unsaved documents were preserved; no application was launched or restarted. Native OS CJK IME delivery, accessibility and GPU presentation remain manual platform validation. File persistence and wire formats are unchanged.
