# Security

Daytrip is pre-release software. Security fixes target the current development
branch; there are no maintained release branches yet.

Use the repository's **Security → Report a vulnerability** form for a private
report when enabled. If that form is unavailable, open an issue asking for a
private reporting channel without including exploit details, credentials, or
private documents. Include a minimal synthetic reproduction, affected revision,
platform, expected behavior and observed impact in the private report.

## Boundaries

Markdown files, image URLs, clipboard contents and imported collaboration state
are untrusted input. Preserve source and checkpoint size limits, bounded decoding,
file conflict checks, symlink guards, transaction validation and export escaping.
Remote images in Live view are fetched automatically; opening an untrusted
document can make network requests. HTML export leaves remote images remote.

Checkpoints and recovery copies can retain deleted text. They are local data,
not an encrypted vault; deleting text from the visible Markdown does not erase
it from shared history. See [collaboration retention](docs/collaboration.md#bounds-and-retention)
and [recovery behavior](docs/writing-workflows.md#recovery-and-export).

## Dependency review

Run `mise run audit` against current RustSec data and `mise run check-secrets`
against a full clone before publication. Keep the application lockfile committed.
The [dependency health report](docs/dependency-health.md) records known transitive
warnings and their scope. Audit warnings stay visible; a passing audit is not a
claim that every dependency is maintained or free from defects.

The [vendored Yrs patch](vendor/README.md) has its own provenance and validation
requirements. Review its upstream releases manually; automated dependency updates
must not silently replace that patch.
