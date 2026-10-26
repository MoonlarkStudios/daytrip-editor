# Dependency health

Reviewed on 2026-10-25 against the application `Cargo.lock` and current RustSec
database. Reproduce with `mise run audit`; advisory data can change independently
of this repository. `mise run check-deps` checks unused direct dependencies, while
strict Clippy checks application warnings. Neither proves that every reachable
code path is necessary or correct.

## Lockfile repairs

The publication review updated three compatible transitive dependencies:

| Dependency | Previous | Selected | Reason |
| --- | --- | --- | --- |
| `rustls-webpki` | 0.103.15 | 0.103.16 | [RUSTSEC-2026-0382](https://rustsec.org/advisories/RUSTSEC-2026-0382.html): reject distrusted certificate intermediates |
| `webpki-roots` | 1.0.9 | 1.0.10 | [RUSTSEC-2026-0381](https://rustsec.org/advisories/RUSTSEC-2026-0381.html): remove the affected trust root |
| `clipboard-win` | 5.4.1 | 5.4.3 | Replace the yanked Windows clipboard release used by Arboard and Iced |

The repaired application lockfile has no vulnerability or yanked-package findings
in this audit. The warnings below remain visible; there is no advisory ignore list.
`mise run audit` fails on vulnerabilities and yanked releases. Review informational
warnings when changing dependencies; a zero exit status does not dismiss them.

## Transitive warnings

| Dependency | Introduced through | Follow-up |
| --- | --- | --- |
| `bincode`, `yaml-rust` | Iced highlighter → two-face → syntect | Track maintained serialization/grammar dependencies in upstream highlighting |
| `paste` | Iced WGPU → wgpu-hal → metal | Track the graphics backend's macro dependency |
| `smallstr` | Vendored Yrs | Review alongside the next explicit Yrs rebase |
| `ttf-parser` | Iced font/shaping dependencies | Track upstream font parser migration |
| `zbus_names`, `zvariant`, `zvariant_derive` | Linux portal/D-Bus dependencies | Track upstream migration to the consolidated zbus API |

These are maintenance notices, not vulnerability findings. They are not direct
application dependencies that can simply be deleted or substituted in `Cargo.toml`.
Use `mise exec -- cargo tree --locked --target all -i PACKAGE` to inspect a path.

The additional [LRU panic-safety advisory](https://rustsec.org/advisories/RUSTSEC-2026-0253.html)
affects `lru 0.16.4`, through `iced_wgpu → cryoglyph 0.1.0`. RustSec classifies it
as an informational unsoundness warning. The documented trigger needs a key whose
destructor panics during `pop`, followed by reuse after catching the panic.
The locked cryoglyph glyph atlas uses cosmic-text's `Copy` `CacheKey` and evicts
with `pop_lru`; that key cannot implement `Drop`. This inspection does not identify
the advisory's trigger in this consumer. The warning remains visible, and the
patched `lru >=0.18.2` requires an upstream compatible dependency change. Recheck
the consumer when updating Iced/cryoglyph rather than forcing an unreviewed major
dependency override.

## Vendored Yrs validation

The [vendor policy](../vendor/README.md) keeps imported implementation files and
upstream licenses intact. Reversing `vendor/yrs.patch` reproduced all 65 original
file checksums in `vendor/yrs-upstream.sha256` during this review. The checked-in
test lockfile and license are intentional additions to the registry distribution.

The additional upstream randomized array suite produced a pending-update assertion
failure in `types::array::test::fuzzy_test_300`. This is separate from the passing
application tests. A pristine copy with the patch reversed failed the same assertion
in two of five full-suite runs (three passed), establishing that the upstream suite
also has this intermittent failure. Original failed-run seeds were
`1382735396790199516` and `3456677333682617693`; pristine failures reported
`1405715979555105946` and `16638136266955145747`. Replaying the first seed alone
passed 20 times in each version, so a seed alone was not sufficient to reproduce
the failure. [Diagnostic excerpts](validation/repository/yrs-array.txt) retain the
assertions. The test remains enabled; do not treat a subsequent random pass
as proof that the failure is resolved. The application admits bounded plain-text
collaboration updates rather than arbitrary shared arrays, but the upstream result
still needs to be considered when maintaining the dependency patch.
