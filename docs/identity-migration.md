# Koan identity and compatibility

The application is **Koan**, with lowercase `koan` in its outlined wordmark.
The Cargo package and executable are `koan`; the macOS bundle is
`target/Koan.app` with identifier `app.koan.editor`. The source repository remains
[MoonlarkStudios/daytrip-editor](https://github.com/MoonlarkStudios/daytrip-editor).

## Existing Daytrip data

The Koan rename needs no data migration. Preferences deliberately continue using
`ProjectDirs::from("app", "Daytrip", "Daytrip")`. The default configuration path is
`~/Library/Application Support/app.Daytrip.Daytrip` on macOS,
`$XDG_CONFIG_HOME/daytrip` or `~/.config/daytrip` on Linux, and
`%APPDATA%\Daytrip\Daytrip\config` on Windows. The complete directory includes
preferences, recovery entries, bundled samples and their assets.

`KOAN_CONFIG_DIR` overrides that directory; `DAYTRIP_CONFIG_DIR` remains a fallback
when the Koan override is unset. An explicit override selects one complete data
location; it does not copy or merge another directory. Use a temporary directory
for isolated validation.

Document metadata keeps its existing names: `.note.md.daytrip`,
`.note.md.daytrip-journal` and `.note.md.daytrip-lock`. Recovery entries retain
`.daytripdraft`. Keeping the shared checkpoint and lock namespace preserves
collaboration identity and the existing file guards across the name change.
Binary signatures, checksums, document UUIDs, CRDT updates and in-memory
`daytrip/` transaction labels are unchanged. No user files are renamed or removed.

New offline exports suggest `.koanreplica`. Both `.koanreplica` and
`.daytripreplica` appear in the import/open filters and use the same binary format.

Benchmark controls are `KOAN_PROFILE_BLOCKS`, `KOAN_PROFILE_SAMPLES`,
`KOAN_PROFILE_COLD_ONLY` and `KOAN_IMPORT_SAMPLES`, with their `DAYTRIP_` equivalents
retained as fallbacks. The Koan setting takes precedence when both are present.
Dated validation records retain the names and commands they recorded; see the
[evidence policy](history.md#application-identity-in-retained-evidence).

## Koan rename validation (2026-10-26)

Validated with the mise-pinned Rust 1.95.0 on macOS/Apple silicon:

```sh
KOAN_CONFIG_DIR=/tmp/koan-rename-check mise run check
mise run check-repo
mise run release
python3 branding/koan/export.py --check
KOAN_CONFIG_DIR=/tmp/koan-rename-preview mise exec -- cargo test --locked theme_ui_previews -- --ignored --nocapture
mise exec -- bash scripts/bundle-macos.sh
codesign --verify --deep --strict --verbose=2 target/Koan.app
```

Formatting, strict Clippy and **187 application tests** passed (16 opt-in tests
ignored). The first sandboxed run could not bind the HTTP image test's loopback
server; the complete check passed with that access enabled. Repository hygiene,
the optimized build, signed bundle identity/icon and all 41 reproducible brand
assets passed. The lockfile changed only the application's package name; dependency
versions and checksums are unchanged.

A regression opens a literal Daytrip checkpoint, preserves its document UUID and
update bytes, and verifies that both names use the same exclusive file lock.
Recovery coverage also checks the retained `.daytripdraft` filename.

The software-rendered theme test produced 21 previews. The Paper and Sage standard
views, Midnight narrow view and brand sheet were visually inspected for the new
wordmark and palette. No native editor was launched, and existing windows and
unsaved documents were untouched. This is branding and compatibility validation,
not a new performance or native accessibility result.

## Earlier development migration (before Daytrip)

The instructions below apply only to installations predating the Daytrip identity.
They are not steps required to move from Daytrip to Koan.

Earlier development installations need a manual transition. Close all editor
windows first and back up the complete configuration directory, Markdown files
and hidden sidecars. Do not run old and new builds against the same document:
their differently named lock files cannot coordinate access.

1. Resolve any interrupted save using the earlier build before migrating. Keep
   conflicting journals and checkpoints for recovery; do not discard them.
2. Copy the earlier configuration directory's contents, including preferences,
   bundled samples, assets and recovery entries, into the new Daytrip location.
   Alternatively, point `DAYTRIP_CONFIG_DIR` to that copied directory. On macOS
   the default is `~/Library/Application Support/app.Daytrip.Daytrip`; on Linux,
   `$XDG_CONFIG_HOME/daytrip` or `~/.config/daytrip`; on Windows,
   `%APPDATA%\Daytrip\Daytrip\config`.
3. For `note.md`, rename the earlier checkpoint sidecar to `.note.md.daytrip`
   and its cooperative lock to `.note.md.daytrip-lock`. Preserve the complete
   files. Never overwrite an existing destination. Normal migration should have
   no pending journal; the current journal name is `.note.md.daytrip-journal`.
4. Rename recovery entries to use `.daytripdraft`, keeping each UUID stem and
   file contents. Exported replica files use the suggested `.daytripreplica`
   extension; their contents remain compatible.
5. Open the migrated copy and confirm its text, collaboration identity, recovery
   list and preferences before making edits. Keep the backup until verified.

Binary checkpoint, journal and recovery signatures and checksums are unchanged;
only their file locations/names change. CRDT writer IDs, document UUIDs, updates,
retained history and relative Markdown assets are preserved by copying complete
files. In-memory transaction/observer labels use `daytrip/`; they are not stored
in the Yjs update format. Do not open a source file before migrating its checkpoint:
without the correctly named metadata, it is treated as an ordinary Markdown import.

Historical text logs use the normalized public identity; see the
[evidence policy](history.md#application-identity-in-retained-evidence).

## Historical Daytrip validation

The rename passed all 184 application tests (16 opt-in profiles ignored), strict
Clippy and formatting checks. The release package and macOS bundle are built from
`daytrip`. Repository links, unused-dependency checks, audit-policy tests and the
four explicitly reviewed maintenance exceptions passed. Source and candidate-tree
scans found no old application-name identifiers; ordinary English uses of the
word in prose and third-party source are unrelated to branding. Numeric values in
all 66 normalized historical logs were compared with the preceding commit and
were unchanged. See [dependency validation](dependency-health.md) for the separate
Yrs, dependency and native-dialog checks.
