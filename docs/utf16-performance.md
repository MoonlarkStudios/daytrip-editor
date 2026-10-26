# Cached Yrs UTF-16 traversal

The subsequent [offline import optimization](offline-imports.md) supersedes the remote-batch costs below.

Still pins Yrs 0.28.0 with a narrow dependency patch that reuses existing item lengths. `Item.len` already records UTF-16 clock length. Previously `content_len(Utf16)` recounted every scalar of the retained string while traversing text, including retained suffixes in edit deltas and prefixes in stable-position capture/resolution. Successful item merging also recounted the concatenated text. The patch returns the cached length for UTF-16 and adds the two cached lengths on merge. Byte lengths keep their original path. Construction, splitting, trimming, and GC already maintain this invariant.

This removes repeated **scalar counting**, not traversal of the item list, conversion within an item during splitting, or copying split string contents. It adds no fields or per-position metadata. Still keeps its UTF-16 Yrs configuration and incremental Ropey byte bridge. Stable positions, origin-scoped undo, history-selection restoration, transient preedit/exactly-once IME commit, persistence format, replica identity, and offline admission paths are unchanged.

The registry version has no equivalent length reuse, so the exact source is retained under `vendor/yrs`, selected by Cargo's `[patch.crates-io]`. The complete [88-line patch](../vendor/yrs.patch), registry archive/original source checksums, MIT license, upstream revision, and upgrade/removal policy are retained in [vendor/README.md](../vendor/README.md). There is one application package and one CRDT backend. The third-party source keeps upstream formatting and existing unsafe internals; the patch and application add no unsafe code. Dependency compilation now exposes upstream warnings that Cargo normally caps for registry dependencies; application Clippy still passes with warnings denied.

## Comparable release measurements

Measured 2026-10-23 on Apple M2 Max / 32 GiB, macOS 26.7.1 build 25G241, aarch64, mise Rust 1.95.0, Iced 0.14, release/thin LTO/codegen-units=1. The fresh baseline is clean commit **81d1f1c**. Only the Yrs dependency override changes production execution; the source/fixture/profile code and all other application dependency versions remain unchanged. [Environment](performance/utf16/environment.txt) and measured source hashes accompany the raw results.

Both runs use the existing 10k/50k rich-block fixtures (910,450 / 4,574,450 bytes), 30 warm samples, Newsreader fonts, 720×800 viewport, persistent widget tree, and software rasterization. Builds and checks completed before serial final measurements; no competing build was run during those measurements. These are event/App/layout/draw/raster CPU totals. They exclude OS/GPU presentation, file IO, decoded images, and actual OS IME delivery. Initialization/first frame and IME/undo/redo each have n=1; do not infer their tail distributions.

Median milliseconds, before → after:

| Interaction | 10k | 50k |
| --- | ---: | ---: |
| cold | 19.952 → 11.875 | 19.162 → 15.487 |
| typing/start | 2.431 → 1.998 | 7.559 → 5.115 |
| typing/middle | 2.944 → 1.788 | 10.051 → 5.021 |
| typing/end | 2.659 → 1.553 | 9.706 → 4.593 |
| typing/syntax-fallback | 2.698 → 1.910 | 9.892 → 5.231 |
| disclosure | 2.759 → 2.913 | 12.423 → 14.210 |
| selection/half-document | 1.261 → 1.340 | 2.155 → 2.253 |
| selection/keyboard | 1.667 → 1.145 | 4.614 → 2.388 |
| selection/drag | 1.899 → 1.498 | 5.088 → 2.696 |
| scroll | 1.984 → 1.352 | 3.890 → 1.385 |
| ime/commit | 7.520 → 2.657 | 11.603 → 6.001 |
| undo/ime | 2.982 → 1.726 | 10.854 → 5.902 |
| redo/ime | 2.769 → 2.895 | 9.938 → 4.988 |

Selected p50 / p95 / maximum totals (ms):

| Interaction | 10k before | 10k after | 50k before | 50k after |
| --- | ---: | ---: | ---: | ---: |
| typing/middle | 2.944 / 3.608 / 4.400 | 1.788 / 2.414 / 2.530 | 10.051 / 10.787 / 17.209 | 5.021 / 7.849 / 9.201 |
| typing/end | 2.659 / 2.946 / 22.104 | 1.553 / 2.061 / 21.013 | 9.706 / 11.062 / 124.231 | 4.593 / 5.372 / 109.674 |
| disclosure | 2.759 / 3.113 / 3.894 | 2.913 / 3.182 / 3.458 | 12.423 / 15.223 / 19.976 | 14.210 / 17.432 / 18.454 |
| scroll | 1.984 / 2.297 / 2.614 | 1.352 / 1.807 / 1.982 | 3.890 / 4.513 / 4.520 | 1.385 / 1.694 / 1.816 |

Middle-typing phase medians (ms):

| Phase | 10k before → after | 50k before → after |
| --- | ---: | ---: |
| crdt | 1.122 → 0.008 | 5.404 → 0.018 |
| source | 0.027 → 0.028 | 0.096 → 0.105 |
| adapters | 0.744 → 0.698 | 3.431 → 3.699 |
| layout | 0.153 → 0.140 | 0.253 → 0.267 |
| raster | 0.875 → 0.863 | 0.857 → 0.881 |

The 50k middle-typing total fell **50.0%**, scrolling **64.4%**. CRDT transaction/coordinate/history median fell **5.404 → 0.018 ms**. Source/projection/native adapter work now dominates warm typing. Phase percentiles are independent; do not sum them to reconstruct a total percentile.

Initialization was **158.7 → 170.7 ms** at 10k and **672.8 → 696.1 ms** at 50k (n=1). This patch targets traversal during interactions; initialization still builds text, parses Markdown and populates native buffers. No startup improvement is claimed. The first trailing-gap insertion still needs full parsing/layout: 50k maximum **124.2 → 109.7 ms**.

The primary disclosure median rose **12.423 → 14.210 ms**. A serial paired repeat found **14.067 → 14.165 ms**; the independent mise profile found 13.997 ms optimized. Thus the initial difference was not reproduced in the paired repeat. Disclosure remains an approximately 14 ms source/projection path at 50k. The repeated pair also confirmed typing **11.251 → 5.302 ms** and scrolling **4.096 → 1.354 ms**. All observations are retained, not pooled into invented sample percentiles. No disclosure speedup is claimed.

## Remote batches, churn, and memory

For 30 incoming eight-edit Unicode batches, 50k total p50/p95/max changed **164.998 / 186.884 / 289.874 → 154.851 / 168.514 / 193.912 ms**; engine admission median **43.613 → 35.352 ms**. Source/cache **26.592 → 26.867 ms** and adapters **78.755 → 78.816 ms** remain. The 10k total median changed **29.645 → 27.702 ms**. Staging, encoded-state traversal, full batch scanning and conservative projection rebuilding still make large offline imports visibly expensive. This patch is not a real-time import latency solution.

The app's 200 paste/undo pairs (400 transactions and rendered frames) at 50k fell **11.163 / 13.479 / 29.894 → 5.049 / 5.762 / 7.517 ms** per operation. At 10k the medians were **3.292 → 1.978 ms**. Persisted checkpoint growth is retained in the raw reports, with unchanged source after churn.

The model benchmark performs 1,000 distributed Unicode insert/delete pairs (2,000 transactions), separate from widget/render latency:

| Measurement | 10k before → after | 50k before → after |
| --- | ---: | ---: |
| Total model churn | 2.746 s → 260.75 ms | 12.570 s → 479.52 ms |
| CRDT portion | 2.668 s → 210.83 ms | 12.247 s → 239.14 ms |
| Source/cache portion | 77.65 → 49.71 ms | 322.29 → 240.18 ms |
| Encoded initial / after churn (both runs) | 910,476 / 989,430 B | 4,574,477 / 4,655,078 B |
| Model reopen after churn | 111.38 → 113.92 ms | 547.24 → 562.81 ms |

At 50k, the source/projection component benchmark's 100 word edits changed **530.66 → 5.10 ms CRDT**, **9.45 → 20.22 ms source cache**, and **223.14 → 260.31 ms projection**. Unicode/syntax: **544.79 → 4.88 / 13.42 → 12.43 / 228.31 → 222.28 ms**. Equal-length: **675.73 → 4.86 / 1.39 → 0.93 / 105.16 → 105.32 ms**. The unmodified cache/projection paths vary between runs, and word-case cache/projection time rose in this observation; their scaling/copy costs remain a concrete next target. These component totals exclude native widgets/rendering.

The core-profile process peak was **664.3 → 684.9 MiB** (+3.1%). The remote/churn process peaked at **680.9 → 659.1 MiB**; process RSS reflects allocator retention and released temporary generations, not isolated CRDT allocation or reclaimed tombstones. After 50k application churn, RSS was **404,224 → 498,832 KiB** in these runs. No memory reduction is claimed. This patch adds no item fields; encoded-state growth is identical in the model benchmark. Retention, no-GC policy, admission bounds, and compatibility are unchanged.

## Raw evidence

- [Baseline core](performance/utf16/before.txt), [final core](performance/utf16/after.txt), [paired baseline repeat](performance/utf16/before-repeat.txt), [paired optimized repeat](performance/utf16/after-repeat.txt).
- [Baseline remote/app churn](performance/utf16/before-remote.txt), [optimized remote/app churn](performance/utf16/after-remote.txt).
- [Baseline component benchmarks](performance/utf16/before-bench.txt), [seven optimized component benchmarks](performance/utf16/bench.txt), [independent mise profile](performance/utf16/mise-profile.txt).
- [Application checks](performance/utf16/check.txt), [upstream tests](performance/utf16/yrs-tests.txt), [release build](performance/utf16/release.txt), [test build](performance/utf16/test-build.txt), [bundle/signature verification](performance/utf16/bundle.txt).


## Correctness and reproduction

All **128 application tests**, seven release component benchmarks, and both profile paths pass. The application regression extends coverage to fragmented Unicode items: cached and fresh captures with both affinities, byte resolution beyond the 32-entry position cache, reversed selection spanning rich blocks, a composition target rebased by a peer, local undo/redo after incoming edits, duplicate delivery and compatible reopen. Existing tests cover three-replica randomized and delayed/out-of-order convergence, contextual disclosure, viewport/drag/wrap anchors, independent human/agent history and Source/Live native-widget exactly-once IME behavior. Fresh source materialization and parsing remain test oracles outside timed paths.

The dependency regression compares cached and freshly counted lengths at every valid scalar split and subsequent squash of text containing CJK, combining characters, ZWJ emoji, RTL and CRLF, checking UTF-16 and byte modes. It also checks non-text/non-countable item lengths. The upstream suite passes **378 tests**, with one upstream ignored test and seven external-asset tests explicitly excluded: five edit-trace files and two dataset files are absent from the published crate. Those assets were not substituted or their tests described as passing.

```sh
mise run check
mise run check-yrs
mise run release
mise exec -- cargo test --locked --release --lib --no-run
# Use the executable emitted above; this checkout emitted:
still_profile_exe=target/release/deps/still-508656e959175f01
/usr/bin/time -l "$still_profile_exe" large_document_interactions --ignored --nocapture --test-threads=1
/usr/bin/time -l "$still_profile_exe" large_document_collaboration --ignored --nocapture --test-threads=1
RUST_TEST_THREADS=1 mise run bench
mise run profile
```

The fresh baseline used `target/release/deps/still-0360e0d741012cb4` built at 81d1f1c. Its core profile, remote/app churn and all component benchmarks were captured before rebuilding. `initial-optimized.txt` retains an exploratory optimized run before restoring Cargo's unrelated tempfile/getrandom lockfile resolution to match baseline; the final comparison uses `after.txt`. No timing thresholds were added to regression tests. Existing windows and unsaved documents were preserved; no editor was launched or restarted. Actual platform CJK IME delivery, GPU presentation and accessibility remain manual validation.
