# CRDT integration measurements

The subsequent [UTF-16 traversal optimization](utf16-performance.md) supersedes the residual traversal costs below; this report preserves the foundation baseline.

Measured on 2026-10-22–23 on an Apple M2 Max, 32 GiB RAM, macOS 26.7.1, mise Rust 1.95.0, Iced 0.14, release/thin LTO/codegen-units=1. Baseline: clean `main` at **`fecbc48`**. After: the CRDT foundation change containing this report; [source checksums](performance/crdt/source-sha256.txt) identify the measured source exactly. Full [environment details](performance/crdt/environment.txt) are retained.

The integration adds measurable text-engine and stable-position costs. It preserves the earlier source/projection optimizations and bounded resident rich layout. It does not make typing faster than the source-only baseline.

## Comparable editor interactions

The existing 10k/50k fixtures, 910,450/4,574,450 source bytes, 720×800 viewport, Newsreader fonts, persistent widget tree, and software rasterizer are unchanged. Warm cases use 30 samples. Cold layout and IME/undo/redo are single observations; distant navigation uses 10 jumps. Full reads and parsing comparisons are correctness oracles outside interaction timers.

These are event/App/layout/draw/software-raster totals, not OS input-to-photon latency. File dialogs, file IO, window chrome/outline construction, GPU presentation, networking, decoded images, and actual OS IME delivery are excluded. Source loading is separately timed. Runs were serial after builds completed, with no competing builds.

Median milliseconds:

| Interaction | 10k before | 10k after | 50k before | 50k after |
| --- | ---: | ---: | ---: | ---: |
| cold | 14.526 | 10.811 | 15.481 | 18.596 |
| typing/start | 2.129 | 2.773 | 5.905 | 7.511 |
| typing/middle | 1.863 | 2.987 | 5.511 | 9.979 |
| typing/end | 1.587 | 2.654 | 4.835 | 9.769 |
| typing/syntax-fallback | 1.928 | 2.702 | 5.226 | 10.197 |
| disclosure | 3.003 | 2.735 | 15.527 | 12.786 |
| selection/half-document | 1.303 | 1.286 | 2.262 | 2.166 |
| selection/keyboard | 1.250 | 1.715 | 2.292 | 4.760 |
| selection/drag | 1.493 | 1.878 | 2.791 | 5.227 |
| scroll | 1.351 | 1.912 | 1.422 | 3.968 |
| ime/commit | 3.275 | 3.867 | 6.809 | 11.831 |
| undo/ime | 2.315 | 3.042 | 4.524 | 10.976 |
| redo/ime | 1.954 | 2.707 | 4.786 | 10.200 |

Source/App initialization, before → after, was **164.1 → 157.3 ms** at 10k and **725.1 → 653.2 ms** at 50k, each n=1. These fresh-instance observations are not a cold-boot distribution. The first frame's cost is the `cold` row above, separate from initialization.

Selected p50 / p95 / maximum totals (ms):

| Interaction | 10k before | 10k after | 50k before | 50k after |
| --- | ---: | ---: | ---: | ---: |
| Typing, middle | 1.863 / 2.611 / 2.958 | 2.987 / 3.567 / 4.321 | 5.511 / 7.325 / 7.345 | 9.979 / 11.910 / 16.863 |
| Typing, end | 1.587 / 1.894 / 21.471 | 2.654 / 2.848 / 21.319 | 4.835 / 5.825 / 116.790 | 9.769 / 10.761 / 117.250 |
| Disclosure | 3.003 / 3.421 / 3.553 | 2.735 / 3.045 / 3.858 | 15.527 / 16.985 / 17.493 | 12.786 / 13.493 / 15.329 |
| Scrolling | 1.351 / 1.774 / 1.833 | 1.912 / 2.274 / 2.493 | 1.422 / 1.701 / 1.799 | 3.968 / 4.438 / 4.486 |

The first insertion in the trailing gap still takes the full parser/layout path. Its 50k maximum was 117.3 ms versus 116.8 ms before. This is retained explicitly rather than hidden by the warm median.

## Where the added time goes

Final middle-typing phase medians (ms):

| Phase | 10k | 50k |
| --- | ---: | ---: |
| CRDT transaction, coordinate bridge, history | 1.131 | 5.366 |
| Source cache and block updates | 0.030 | 0.097 |
| App adapters, projection/native synchronization | 0.749 | 3.433 |
| Rich layout | 0.155 | 0.251 |
| Software rasterization | 0.859 | 0.842 |

Phase percentiles are independent; do not add them to reconstruct a total percentile. Adapter time also includes selection anchoring outside the model call. Diagnostic clocks are compiled only into test/profile builds.

Yrs 0.28 recounts UTF-16 lengths while traversing text items, including retained spans in text deltas. Its permanent positions also require UTF-16 clocks. The local Ropey coordinate bridge is incremental, but it cannot eliminate those upstream traversals. This is the main residual document-size-dependent cost: about 5.4 ms per 50k middle-typing transaction here. Stable first-visible-row capture adds scrolling work, bringing the 50k scroll median to 4.0 ms.

An initial integration measured **31.1 ms** for 50k middle typing. Generation-scoped position caching, selective undo-selection metadata, removing duplicate viewport capture, and clearing unrelated visual wrap positions reduced that to **10.0 ms**. [Initial](performance/crdt/initial-integration.txt) and [intermediate](performance/crdt/position-cache.txt) diagnostic runs are retained; those iterations were uncommitted and are not substitutes for the final source comparison.

The source/projection component benchmark separates CRDT from the previous source phase. At 50k, 100 word edits took **542.74 ms CRDT**, **10.38 ms source cache**, and **235.66 ms projection**. Unicode/syntax edits took **544.07 / 10.75 / 213.07 ms**. Equal-length replacements took **681.34 / 1.28 / 104.95 ms**. These are component totals for 100 edits, excluding native widgets/rendering, and should not be described as interaction latency. Existing source/projection splice optimizations remain active.

## Remote batches and sustained churn

The additional profile receives 30 compatible replica states per fixture, each containing eight disjoint Unicode insertions. Peer editing/export happens outside the interaction timer. The timed path includes staged CRDT admission, text deltas, source/cache updates, projection/native synchronization, layout, and software rasterization. It begins after file IO and does not include user time in a dialog.

| Eight-edit remote transaction | 10k | 50k |
| --- | ---: | ---: |
| Total p50 / p95 / max, ms | 28.011 / 34.095 / 37.003 | 158.730 / 165.543 / 190.413 |
| CRDT admission median, ms | 8.125 | 40.827 |
| Source/cache median, ms | 4.571 | 28.138 |
| Adapters median, ms | 12.005 | 74.791 |
| Layout median, ms | 2.365 | 13.352 |

Batched source parsing scans once and preserves unaffected rendered blocks. Projection conservatively rebuilds when its single-window proof cannot cover the batch. Sparse native synchronization avoids replacing all unchanged lines between distant edits. These manual offline imports still produce a visible UI work interval at 50k; repeated real-time delivery would need worker admission and broader incremental projection proofs. Real-time UI is outside this delivery.

The actual app then performs **200 paste/undo pairs (400 transactions and frames)**. At 50k their combined per-operation total was **11.227 / 12.423 / 15.597 ms**. Its checkpoint grew **4,580,919 → 4,585,159 bytes** (+4,240 bytes), with unchanged source. At 10k: **916,791 → 921,029 bytes** (+4,238). This includes live projection, native content, layout, and rasterization.

A separate model benchmark performs **1,000 insert/delete pairs at 1,000 deterministically dispersed paragraph locations**, ending with the exact original source:

| Model churn, 2,000 transactions | 10k | 50k |
| --- | ---: | ---: |
| Total elapsed | 2.624 s | 12.492 s |
| CRDT portion | 2.577 s | 12.273 s |
| Source/cache portion | 46.43 ms | 218.48 ms |
| Encoded state, initial → after | 910,476 → 989,430 B | 4,574,477 → 4,655,078 B |
| Checkpoint after churn, including frame | 989,494 B | 4,655,142 B |
| Reopen model from encoded state | 110.69 ms | 540.65 ms |

Reopen and model initialization are component observations, including materialization/indexing/parsing, rather than complete app startup. Deleted payload and causal information are retained. Checkpoints intentionally duplicate visible Markdown in encoded form so the ordinary `.md` remains usable; journal publication temporarily needs another checkpoint-sized record.

## Memory and limits

The comparable direct core-profile process peak was **661.3 → 680.1 MiB** (+2.9%) across both fixtures. This includes the editor model, source/projection/native buffers, software renderer, and allocator retention; it is not isolated CRDT allocation.

The remote/app-churn run peaked at **680.6 MiB**. In-process 50k RSS was **597,808 KiB** after remote batches and **485,104 KiB** after app churn. The model-only benchmark reported **308,416 → 285,088 KiB** across distributed churn. RSS can fall as temporary allocations and old layout/projection generations are released; that does not imply tombstones were reclaimed. Encoded-state growth above is the reliable retained-history measurement. Raw snapshots for both fixture sizes are available.

The current bounds are 16 MiB text, 64 MiB encoded state, 200,000 retained structures/ranges/clients, 65,536 source edits per transaction, and 4,096 edits per agent proposal. Large batches and near-limit history operations may be conservatively refused. No text/operations are silently dropped and no lossy compaction occurs. Large proposals, single exceptionally long paragraphs, full-parser dependencies, and UTF-16 traversals remain CPU scaling limits. Maximum-size/batch-limit inputs are not latency guarantees.

## Reproduction and evidence

```sh
mise run check
mise run release
mise exec -- cargo test --locked --release --lib --no-run
# Use the executable path emitted above. This checkout emitted:
daytrip_profile_exe=target/release/deps/daytrip-0360e0d741012cb4
/usr/bin/time -l "$daytrip_profile_exe" large_document_interactions --ignored --nocapture --test-threads=1
RUST_TEST_THREADS=1 mise run bench
/usr/bin/time -l "$daytrip_profile_exe" large_document_collaboration --ignored --nocapture --test-threads=1
mise run profile
```

- [Fresh baseline](performance/crdt/before.txt), [final direct interaction measurement](performance/crdt/after.txt).
- [Seven serial release benchmarks](performance/crdt/bench.txt), [remote/application churn and memory](performance/crdt/remote-and-churn.txt).
- [Independent mise profile task validation](performance/crdt/mise-profile.txt); its second run is retained separately from the direct comparison.
- [Required check](performance/crdt/check.txt), [release build](performance/crdt/release.txt), [release test build](performance/crdt/test-build.txt).

All 127 ordinary tests, seven release benchmarks, and both profile paths passed. No machine-dependent timing assertions were added. Actual OS CJK IME delivery in Live/Source, accessibility, GPU presentation, and Windows/Linux filesystem/lock/durability behavior still require manual platform validation. Existing native windows and unsaved documents were preserved throughout this work.
