# Pinned Yrs patches

`yrs/` is the crates.io **yrs 0.28.0** source distribution, MIT licensed.
Its `.cargo_vcs_info.json` identifies upstream commit
`23b7f5693bbf9e7d26340c521ee8647f79bdfba2`, path `yrs` in
https://github.com/y-crdt/y-crdt. `yrs/LICENSE` is the license from that commit.
The registry archive SHA-256 and original file hashes are in
`yrs-upstream.sha256`; `yrs.patch` records all local source and manifest changes.
The dependency's test-only `Cargo.lock` is separately resolved and committed for
reproducible upstream checks. Cargo's root `[patch.crates-io]` selects this dependency; Still remains one
application package, without a workspace or second CRDT backend.

The patch reuses `Item.len`, the existing UTF-16 clock length, in
`Item::content_len(Utf16)` and adds cached lengths on successful item squash.
Construction, split, trim, and garbage collection already maintain this invariant.
Byte indexing still delegates to the original content length. No extra fields,
unsafe code, wire format, text indexing units, or GC policy are added.
Upstream's existing unsafe implementation remains third-party dependency code;
Still's application code and this patch remain safe Rust. Do not mechanically
apply Still's formatting/lint conventions to the imported dependency. Git whitespace
checks exclude the imported files and patch context to preserve them verbatim;
raw profile logs likewise retain exact tool output.

## Dependency diagnostics

With Still's pinned Rust 1.95 toolchain and `sync` feature, the upstream library
emits 58 warnings in six categories: `dead_code`, `unused_imports`, `unused_mut`,
`unused_variables`, `unused_assignments`, and `mismatched_lifetime_syntaxes`.
Cargo shows these because the patch is a path dependency; non-path dependencies
normally have their lints capped ([Cargo lint configuration](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section)).

The vendored manifest allows only these observed categories for Yrs. This is an
exception for imported third-party code so we can preserve upstream implementation
and formatting while keeping application build output useful. It suppresses the
diagnostics rather than repairing upstream code. Other lint categories, including
`unused_must_use`, remain enabled, and Still's own lint policy and warnings-denied
Clippy check are unchanged. Do not add a blanket `warnings` or `unused` allowance
or global Rust flags. Reassess this list when rebasing or removing the dependency
patch; remove entries when the corresponding upstream warnings are resolved.

The cached-length regression is beside the upstream block tests. Still's ordinary
model/native-widget tests cover the integration, Unicode anchors, undo/redo,
IME and replica convergence. The additional upstream suite is run separately
with its own manifest/lockfile; commands and asset exclusions are in
`docs/utf16-performance.md`. Root `mise run check` tests the application rather
than automatically testing dependency internals.

The second patch fixes pending updates after a sparse clock gap is filled.
Missing dependencies now retain their exact operation ID, and retries check that
ID's presence instead of requiring the client's highest clock to advance. It
changes no wire encoding or public method signatures. A deterministic regression
covers Unicode text, all six delivery orders, duplicate delivery, and V1/V2 updates.

On 2026-10-25, the previously intermittent `types::array::test::fuzzy_test_300`
passed in ten consecutive full-suite runs with this fix. The new deterministic
regression fails without the retry fix and passes with it, including the optional
`weak` feature. The array test remains enabled; only the existing seven tests
requiring unpublished upstream assets are excluded. See the
[Yrs repair report](../docs/yrs-pending-updates.md) for the cause, reproduction,
and retained evidence. The upstream test harness still emits seven test-only
`assert_matches2` deprecation warnings; no new lint allowances were added.

Keep this a narrow, reviewable patch. On an upstream release containing an
equivalent fix, remove the vendor override and directory, pin the new version,
then repeat the invariant tests and comparable editor profiles. Until then,
security and correctness upgrades must rebase the patch explicitly and retain
upstream provenance, license, and the original source checksum manifest.
