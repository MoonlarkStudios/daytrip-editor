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

## Follow-up repairs

The follow-up review reduced nine audit findings to four maintenance notices:

| Change | Result |
| --- | --- |
| `rfd 0.16.0 → 0.17.2` | Removes ashpd/zbus and the three notices for `zbus_names`, `zvariant`, and `zvariant_derive` |
| Cryoglyph 0.1.0 manifest backport; `lru 0.16.4 → 0.18.5` | Repairs [the panic-safety unsoundness advisory](https://rustsec.org/advisories/RUSTSEC-2026-0253.html) |
| Iced highlighter 0.14.0 minimal features | Removes unused YAML/plist loaders, including unmaintained `yaml-rust`; preserves embedded syntax/theme loading |

The [vendor policy](../vendor/README.md) records source revisions, checksums,
licenses, patch scope and removal criteria. No renderer or highlighter implementation
was changed. RFD 0.17 uses the native `libdbus` portal path on Linux with a Zenity
fallback; packaging needs those runtime components and a supported desktop portal.
See [RFD's platform requirements](https://docs.rs/rfd/0.17.2/rfd/#linux--bsd-backends).

## Reviewed maintenance exceptions

The current application lockfile has no vulnerability, unsoundness or yanked-package
findings in this audit. Four transitive maintenance notices remain because the
current compatible upstream stack has no released replacement:

| Dependency | Advisory | Consumer and review trigger |
| --- | --- | --- |
| `bincode 1.3.3` | [RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141.html) | Syntect's embedded dumps require this serialization format; recheck Iced/two-face/syntect upgrades |
| `paste 1.0.15` | [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html) | Build-time token generation in metal through WGPU; recheck backend upgrades |
| `smallstr 0.3.1` | [RUSTSEC-2026-0215](https://rustsec.org/advisories/RUSTSEC-2026-0215.html) | Yrs text storage and API types; recheck each explicit Yrs rebase |
| `ttf-parser 0.25.1` | [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) | fontdb and owned_ttf_parser/ab_glyph; recheck font and windowing stack upgrades |

These are approved scope exceptions, not claims that maintenance is unnecessary.
[The policy file](../.cargo/audit-exceptions.json) binds each exception to the exact
advisory, package and version, with a rationale and **2027-01-23** review deadline.
`mise run audit` prints each finding and fails on new or changed findings, expired
or stale exceptions, vulnerabilities, unsoundness, notices and yanked releases.
External cargo-audit ignore lists also fail. Keep the exception list narrow;
changing a dependency version requires another review. Seven synthetic policy tests
cover allowed findings and rejection boundaries in `mise run check-repo`.

[Audit output](validation/dependencies/audit.txt) records the review. RustSec database
revision was `21d909e4c8bd4e14c7aea58bf980c881a0053ab1`; future advisory updates can
change this result. Inspect the full-platform path with
`mise exec -- cargo tree --locked --target all -i PACKAGE`.

## Vendored Yrs validation

The intermittent array-suite failure is repaired by a narrow sparse-clock retry
patch. The [Yrs repair report](yrs-pending-updates.md) explains the deterministic
reproduction, before/after result and ten passing full-suite runs. Original
[diagnostics](validation/repository/yrs-array.txt) remain as historical evidence.
The array test is still enabled and the existing asset exclusions are unchanged.

## Application validation

The macOS/aarch64 run passed 184 application tests (16 opt-in profiles ignored),
strict Clippy, formatting, the release build, repository checks and the audit
policy. Existing syntax tests cover fresh/cached highlighting, Unicode and all
application themes. A fresh native window with isolated preferences rendered the
welcome document and opened/cancelled both native Open and Save As dialogs.
No user document was loaded or written. Hosted macOS, Linux and Windows build/test
results are available in [the public CI workflow](https://github.com/MoonlarkStudios/daytrip-editor/actions/workflows/ci.yml).
Native Linux and Windows dialog behavior still requires manual platform testing.

Release CRDT churn/reopen, 10k/50k-block interactions, and collaboration profiles
passed. Raw outputs: [before interactions](validation/dependencies/before-profile.txt),
[after interactions](validation/dependencies/after-profile.txt),
[before collaboration](validation/dependencies/before-collaboration.txt),
[after collaboration](validation/dependencies/after-collaboration.txt), and
[CRDT benchmark](validation/dependencies/after-bench.txt).

Measurements used Apple M2 Max / 32 GiB, macOS 26.7.1, Rust 1.95.0, release thin LTO,
one codegen unit, and the same fixtures with 10 interaction samples per size
(400 churn steps). Baseline was `15d8baf`; the successor includes the Yrs retry fix,
RFD update and two manifest overrides. No application or fixture code changed.

| 50k-block operation | Before p50 / p95 (ms) | After p50 / p95 (ms) |
| --- | --- | --- |
| Middle typing, total | 6.112 / 9.577 | 6.732 / 9.852 |
| Distant jump, total | 16.213 / 16.989 | 14.992 / 19.172 |
| Remote eight-edit batch, total | 45.542 / 93.917 | 40.463 / 86.311 |
| Application churn, total | 5.667 / 11.422 | 4.929 / 5.692 |

Cold 50k load was 203.2 → 206.5 ms (one sample each). The 2,000-transaction CRDT
benchmark completed in 426.2 ms at 50k, with 89.9 ms reopen. These small, mixed
samples are regression smoke checks, not evidence of a general speedup. Ten samples
provide weak tail estimates; background host work and allocator history vary.
The profile uses software rasterization and does not measure the upgraded GPU
cache, OS input latency or presentation. The native smoke check exercises rendering
but is not a GPU performance benchmark.
