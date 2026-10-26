# Daytrip identity and manual migration

The Cargo package and executable are `daytrip`; the macOS bundle identifier is
`app.daytrip.editor`. Preferences use `ProjectDirs::from("app", "Daytrip", "Daytrip")`
or the `DAYTRIP_CONFIG_DIR` override. Benchmark controls use `DAYTRIP_PROFILE_BLOCKS`,
`DAYTRIP_PROFILE_SAMPLES`, `DAYTRIP_PROFILE_COLD_ONLY` and `DAYTRIP_IMPORT_SAMPLES`.
There are no legacy environment aliases or automatic path migrations.

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

## Validation

The rename passed all 184 application tests (16 opt-in profiles ignored), strict
Clippy and formatting checks. The release package and macOS bundle are built from
`daytrip`. Repository links, unused-dependency checks, audit-policy tests and the
four explicitly reviewed maintenance exceptions passed. Source and candidate-tree
scans found no old application-name identifiers; ordinary English uses of the
word in prose and third-party source are unrelated to branding. Numeric values in
all 66 normalized historical logs were compared with the preceding commit and
were unchanged. See [dependency validation](dependency-health.md) for the separate
Yrs, dependency and native-dialog checks.
