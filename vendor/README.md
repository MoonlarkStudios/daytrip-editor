# Pinned Yrs traversal patch

`yrs/` is the crates.io **yrs 0.28.0** source distribution, MIT licensed.
Its `.cargo_vcs_info.json` identifies upstream commit
`23b7f5693bbf9e7d26340c521ee8647f79bdfba2`, path `yrs` in
https://github.com/y-crdt/y-crdt. `yrs/LICENSE` is the license from that commit.
The registry archive SHA-256 and original file hashes are in
`yrs-upstream.sha256`; `yrs.patch` is the complete source change.
The dependency's test-only `Cargo.lock` is separately resolved and committed for
reproducible upstream checks. Cargo's root `[patch.crates-io]` selects this dependency; Still remains one
application package, without a workspace or second CRDT backend.

The patch reuses `Item.len`, the existing UTF-16 clock length, in
`Item::content_len(Utf16)` and adds cached lengths on successful item squash.
Construction, split, trim, and garbage collection already maintain this invariant.
Byte indexing still delegates to the original content length. No extra fields,
unsafe code, wire format, algorithms, text indexing units, or GC policy are added.
Upstream's existing unsafe implementation remains third-party dependency code;
Still's application code and this patch remain safe Rust. Do not mechanically
apply Still's formatting/lint conventions to the imported dependency. Git whitespace
checks exclude the imported files and patch context to preserve them verbatim;
raw profile logs likewise retain exact tool output.

The cached-length regression is beside the upstream block tests. Still's ordinary
model/native-widget tests cover the integration, Unicode anchors, undo/redo,
IME and replica convergence. The additional upstream suite is run separately
with its own manifest/lockfile; commands and asset exclusions are in
`docs/utf16-performance.md`. Root `mise run check` tests the application rather
than automatically testing dependency internals.

Keep this a narrow, reviewable patch. On an upstream release containing an
equivalent fix, remove the vendor override and directory, pin the new version,
then repeat the invariant tests and comparable editor profiles. Until then,
security and correctness upgrades must rebase the patch explicitly and retain
upstream provenance, license, and the original source checksum manifest.
