# Source and projection updates

Measured on 2026-10-22 with mise's Rust 1.95.0, locked Iced 0.14 / pulldown-cmark 0.12, release optimization, macOS 26.7.1 on Apple silicon (arm64), and bundled Newsreader fonts. The baseline is `740fc00`, after [native buffer snapshots](native-snapshots.md).

## Ownership and update boundaries

The source model already edited its String in place and stored patch-based undo. The projection still copied the entire source and displayed text after each accepted edit, deeply cloned parsed metadata, and rebuilt rich structures containing image strings and nested table rows. Its immutable source snapshot was also retained by the widget solely to identify content changes.

The projection now owns reusable source/display Strings and parsed vectors. A separate small `Arc<()>` identifies a source revision; disclosure preserves that identity, while every source update gets a new one. The layout retains this identity instead of retaining source text. Source edits preflight their fast-path conditions before mutating the projection, and rejection leaves it unchanged for the existing parser fallback.

Word edits splice both Strings and update retained metadata in place. Offsets before the edit remain unchanged; equal-length replacements skip all offset rebasing and share the unchanged mapping and line-start snapshots. Rich headings, images and tables use sorted ranges to locate the relevant suffix. Other parsed vectors still scan their ranges, rebasing only those reaching the edit or its suffix.

Certified block updates parse the same isolated windows established by the source model. They validate metadata containment and ordering, then splice local records into owned vectors. Unchanged image strings, table rows and other payload allocations survive. Source and displayed table cells retain their separate coordinate systems. Physical line starts are patched from the local result instead of rediscovered by scanning the full displayed text. The source model also skips block-offset rebasing for equal-length edits.

Old layout still needs immutable mappings, highlight revisions and line starts for hit testing, viewport anchoring and predecessor validation. Length-changing edits produce successor snapshots. Mapping construction now sizes one output buffer and copies its prefix/local/suffix directly, removing temporary vectors and repeated growth. Even when offsets are shared, a fresh highlight revision prevents a skipped intermediate update from authorizing stale geometry. Source selections, native content, history, IME state and widget identity stay intact.

## Interaction results

The unchanged mixed fixtures contain 10k/50k blocks and 910,450/4,574,450 source bytes. Both versions run in fresh test processes, serially across both sizes, with the same 720×800 viewport, persistent widget tree and software renderer. Compilation and other repository builds/tests are excluded from each profile. Ordinary interactions have 30 samples; distant jumps have 10. Totals include event dispatch, application update, layout, draw submission and software rasterization, in milliseconds, p50 / p95.

| Interaction | 10k before | 10k after | 50k before | 50k after |
| --- | ---: | ---: | ---: | ---: |
| Typing / middle | 2.58 / 2.80 | 1.72 / 2.19 | 10.10 / 10.89 | 4.72 / 6.00 |
| Typing / end | 2.36 / 2.96 | 1.57 / 1.81 | 9.83 / 12.13 | 4.61 / 5.48 |
| Local syntax edit | 3.81 / 4.27 | 1.79 / 2.53 | 25.36 / 27.84 | 4.60 / 6.62 |
| Disclosure | 2.97 / 3.38 | 2.78 / 3.15 | 14.37 / 16.01 | 13.65 / 15.00 |
| Half-document selection | 1.28 / 1.54 | 1.30 / 1.44 | 2.23 / 2.51 | 2.23 / 2.39 |
| Keyboard selection | 1.20 / 4.98 | 1.16 / 5.21 | 2.18 / 26.06 | 2.04 / 26.41 |
| Drag selection | 1.54 / 3.69 | 1.49 / 3.68 | 2.53 / 15.01 | 2.52 / 15.73 |
| Scrolling | 1.34 / 1.69 | 1.40 / 1.76 | 1.41 / 1.83 | 1.30 / 1.67 |

At 50k, middle typing is about 53% faster and the local syntax series about 82% faster. Median application update falls from 8.92 to 3.60 ms for middle typing and 23.98 to 3.45 ms for syntax edits; layout remains about 0.25 ms. The historical `typing/syntax-fallback` label includes eligible block parsing, so this improvement does not describe every Markdown structural edit.

One widget composition/history cycle runs per size. Assertions check transient preedit, exactly one Japanese/combining-character/emoji commit, and exact source/selection after undo and redo. These are single observations rather than latency distributions:

| Action | 10k before → after | 50k before → after |
| --- | ---: | ---: |
| Unicode commit | 5.24 → 5.35 ms | 25.25 → 4.97 ms |
| Undo commit | 4.21 → 1.90 ms | 24.68 → 4.37 ms |
| Redo commit | 3.52 → 1.86 ms | 25.63 → 4.42 ms |

The 10k commit's application update falls from 2.84 to 0.80 ms, but its software raster sample rises from 1.24 to 3.90 ms. This illustrates why model improvements and total single-event times must be distinguished. Host load, allocator history and caching also vary across runs.

Selection, scrolling and disclosure-only behavior are broadly unchanged. Disclosure still projects document-wide text/maps, and selection that changes rich geometry retains substantial tails. The first insertion into trailing blank space still takes full parsing and layout: the 50k end-typing maximum is 122 ms, versus 118 ms before. There is no worst-case frame-time guarantee.

Load is essentially unchanged: 166 → 156 ms at 10k and 676 → 669 ms at 50k, each a single observation. The complete two-size process peaks at about 874 → 660 MiB RSS. This includes allocator retention, shaped caches and both fixtures; it is not isolated window memory or a guaranteed 214 MiB reduction. Native shaping-cache limits remain unchanged.

## Source/projection diagnostic

The new diagnostic runs 100 edits per case on the same mixed fixtures and separates source-model time from projection time. Initialization and final correctness checks are excluded. Word and Unicode/syntax cases insert at a moving head; the equal-length case alternates ASCII replacements. The following run is serial and isolated from other tests; values cover all 100 edits.

| Blocks / edit | Source model | Projection | Source mirror splice bytes | Equivalent full source bytes |
| --- | ---: | ---: | ---: | ---: |
| 10k / word | 4.16 ms | 46.49 ms | 100 | 91,050,050 |
| 10k / Unicode/syntax | 3.57 ms | 42.45 ms | 37,300 | 91,065,200 |
| 10k / equal length | 0.53 ms | 19.86 ms | 100 | 91,045,000 |
| 50k / word | 14.43 ms | 253.03 ms | 100 | 457,450,050 |
| 50k / Unicode/syntax | 11.94 ms | 230.19 ms | 37,400 | 457,465,200 |
| 50k / equal length | 0.72 ms | 101.57 ms | 100 | 457,445,000 |

The byte counter measures replacement input to the projection's retained source String. The equivalent column sums the source sizes that the previous implementation copied into immutable snapshots. It excludes initial construction, String suffix moves/capacity growth, displayed text, immutable mapping/index copies, parser input and native buffer storage. It is not a total allocation or copied-byte count. Final source, text, styles and rich structures match full projection; all unaffected image URL allocations remain identical. The small splice counts do not imply constant-time updates: suffix movement, metadata scans and shared snapshots still scale with document size.

## Validation and remaining work

`mise run check` passes formatting, Clippy with warnings denied, and 96 regression tests; eight benchmarks/profiles are ignored by default. All six release benchmarks, the complete 10k/50k interaction profile, and `mise run release` pass. The macOS bundle is rebuilt and ad-hoc signed without opening or restarting existing windows.

Full-parse differential tests compare accepted word/block updates with text, styles, reusable parsed metadata, rich structures, line starts, disclosure and every UTF-8 source/display mapping in both affinities. The block mutation matrix accepts 1,180 updates across headings, tables, images, containers, references, footnotes and uncertain boundaries, including CRLF and EOF table decorations. Uncertain cases retain fallback parsing. Ownership tests verify retained source/display buffers and table/image payloads, unchanged old layout snapshots, equal-length map/index sharing and fresh predecessor identity. Virtual geometry tests include skipped intermediate updates followed by equal-length replacement, repeated reflow, Unicode input and topology changes.

Existing application and widget tests cover cursor motion, forward/reverse cross-block selections, table/image editing, disclosure, scroll anchoring, wrap affinity, source/live switching, clipboard, history and transient IME composition. No dependencies, public library APIs, history semantics, document limits or theme behavior change.

Contiguous Strings still move suffixes and occasionally reallocate. Length-changing updates still copy immutable mapping/highlight/index arrays, and metadata suffixes still rebase. Disclosure-only projection, context-sensitive parser fallbacks, large cross-block edits, very long physical lines and outline construction remain scaling limits. General incremental CommonMark parsing remains future work. Actual native CJK IME integration, OS input delivery, GPU presentation, accessibility, file I/O and image decoding are outside this software harness. Existing windows and unsaved documents were preserved.

## Reproduce and evidence

```sh
mise run check
mise run bench
mise run profile
mise run release
mise exec -- cargo test --locked --release --lib --no-run
```

Run the emitted executable directly with its actual hash to exclude compilation:

```sh
/usr/bin/time -l target/release/deps/dosty-<hash> large_document_interactions --ignored --nocapture --test-threads=1
target/release/deps/dosty-<hash> performance_source_projection_updates --ignored --nocapture --test-threads=1
```

The baseline was measured before edits. To reproduce it, archive `740fc00` into a temporary directory, build through mise from the trusted repository with `--manifest-path` pointing there, and run its executable before rebuilding the current version. A shared `CARGO_TARGET_DIR` reuses dependencies; executable filenames can be identical. The interaction fixture, input sequence and sample count are unchanged. The component diagnostic is new; `mise run bench` runs all six benchmarks concurrently by default, so its component timings are retained separately from the serial diagnostic.

- [Baseline interactions and process metrics](performance/source-projection/before-interactions.txt).
- [Final interactions and process metrics](performance/source-projection/after-interactions.txt).
- [Serial source/projection diagnostic](performance/source-projection/source-projection-component.txt).
- [All release benchmarks](performance/source-projection/model-layout.txt).
