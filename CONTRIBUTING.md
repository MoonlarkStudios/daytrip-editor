# Contributing to Daytrip

Daytrip is a native Rust/Iced application in one Cargo package. Prefer direct code, clear ownership, and changes whose behavior and cost can be reviewed independently. The Cargo package and executable are named `daytrip`.

## Development environment

Install [mise](https://mise.jdx.dev/getting-started.html) and your platform's native build tools:

- **macOS:** Xcode Command Line Tools.
- **Linux:** a C toolchain and X11/Wayland development libraries. On Debian/Ubuntu,
  install `build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-xcb-dev`.
  File dialogs also need `libdbus`, a GTK/GNOME/KDE desktop portal backend, and
  Zenity for fallback.
- **Windows:** MSVC C++ build tools.

From the repository root:

```sh
mise trust
mise install
mise run dev
```

`mise.toml` pins Rust, rustfmt, and Clippy. Run Cargo through mise; keep `Cargo.lock` committed and use `--locked` for builds and tests. Change dependencies and the lockfile together when an update is intentional.

| Command | Purpose |
| --- | --- |
| `mise exec -- cargo fmt --all` | Apply formatting |
| `mise run check` | Check formatting, run Clippy with warnings denied, run regression tests |
| `mise run check-repo` | Check local documentation links/anchors, unused dependencies, workflow syntax and audit policy tests |
| `mise run check-secrets` | Scan all reachable Git history for credentials; use a full clone |
| `mise run audit` | Check locked dependencies against current RustSec data, rejecting unapproved findings and expired exceptions |
| `mise run check-yrs` | Validate the pinned Yrs dependency patch when changing it |
| `mise run release` | Build the optimized application |
| `mise run bench` | Run the ignored model and layout benchmarks |
| `mise run profile` | Profile 10k/50k-block interactions through software rasterization |
| `mise exec -- bash scripts/bundle-macos.sh` | Build and ad-hoc sign `target/Daytrip.app` on macOS |

Set `DAYTRIP_CONFIG_DIR` to a temporary directory for isolated manual testing. Preserve existing windows and unsaved documents when testing another build.

Repository checks use task-scoped tool versions pinned in `mise.toml`; mise installs them on first use. They do not add application dependencies. Audit and policy tests also require Python 3.9 or newer. Link checks are offline and cover repository documentation, including heading anchors; Markdown regression fixtures and runtime sample paths are not documentation links. Advisory checks need network access and leave informational warnings visible. See [dependency health](docs/dependency-health.md) for the current review.

## Source layout and API

```text
src/
  main.rs       Thin executable calling daytrip::run()
  lib.rs        Module declarations and deliberate public exports
  app/          State, messages, commands, views, native startup
  document/     Source, selection, projection, rich metadata, table edits
  editor/       Iced widgets, rich layout, viewport behavior
  theme/        Semantic tokens, theme resolution, component styles
  io/           File storage and background image loading
tests/
  fixtures/     Shared Markdown regression inputs
docs/           Architecture, evaluation, and measured performance evidence
```

See [architecture](docs/architecture.md) for the module map and current boundaries. These groups describe responsibilities; application messages may cross them. Split files when a coherent responsibility warrants it. Introduce traits, generic interfaces, or additional crates only for a concrete need. A workspace can follow a demonstrated dependency or build-time benefit.

Keep implementation modules private or crate-visible. The public library entry point is currently `run()`. Add public APIs for real consumers; test private behavior with unit tests beside its implementation. Use `tests/` for integration tests only when they exercise an intentional public API.

## Rust conventions

Follow the applicable [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html), with the project exceptions below. Stable Rust, rustfmt, and the lints in `Cargo.toml` define the automated baseline; unsafe code is forbidden.

- Use descriptive names, small cohesive functions, and ordinary Rust types. Document invariants, coordinate systems, and reasons for non-obvious decisions.
- Prefer borrowing and explicit ownership. Accept `&str` and slices when ownership is unnecessary. Clone only when the ownership or snapshot boundary requires it.
- Use enums and named types to make meaningful states explicit. Keep visibility as narrow as practical.
- Return errors for expected external failures. Use typed errors when callers need to distinguish cases, and add useful context at the UI or logging boundary. Reserve panics for violated invariants; explain production `expect` calls. Tests may use `unwrap` for setup and assertions.
- Avoid blanket lint allowances, unnecessary abstractions, and speculative optimization. Explain any local lint exception at its narrowest scope.

### Project exceptions and interpretation

- **Cargo layout:** use the responsibility-based directories above. Chromium's flat source layout, small-crate preference, and build/integration rules do not govern this application.
- **Application runtime:** Iced tasks and Tokio background work are supported. Keep blocking file work, network requests, and image decoding outside the UI event path, with explicit resource limits.
- **Shared snapshots:** `Arc` is appropriate for immutable source snapshots, highlight arrays, shaped paragraphs, and asynchronous payloads where shared ownership or identity is required. Avoid copying whole buffers to satisfy borrowing when a borrowed snapshot suffices.
- **Imports:** grouped imports of Iced widget helpers are accepted for readable declarative views. Private unit-test modules may use `use super::*`; production imports should name their dependencies.
- **Module roots:** prefer declarations and deliberate re-exports in `lib.rs` and group `mod.rs` files. Native startup lives in `app/startup.rs`, with a small executable bootstrap in `main.rs`.

Vendored dependencies retain upstream formatting and existing implementation details;
Daytrip's safe-Rust requirement applies to application code and our dependency patch.
The narrow dependency patches, their provenance and removal policies are in
[vendor/README.md](vendor/README.md). Run `mise run check-yrs` when changing it;
do not apply application lint cleanup to imported third-party source.

Record additional guide exceptions here when they become necessary, with their reason.

## Editing and theme invariants

`Document` owns the authoritative Yrs Markdown text and its derived source cache. All source edits enter its validated transaction path. See [collaboration](docs/collaboration.md) for byte/UTF-16 conversion, stable positions, scoped history and persistence. Selections use UTF-8 byte offsets with distinct anchor and head positions. Source/display mappings must preserve selection direction, hidden delimiters, atomic images, table structure, and undo history.

Contextual disclosure reveals the active heading prefix and the inline span being edited. Concealed syntax occupies no reserved space. Local reflow is allowed; source caret position, selection, wrap affinity, and the viewport anchor must survive it. Drawing uses measured layout and must not parse Markdown.

Themes follow definition → resolved semantic tokens and `iced::Theme` → centralized styles and drawing. Resolve a preset or user file on load or theme switch and store the result in `App`. `App::theme` returns the stored Iced theme handle. Widgets and canvases consume the same resolved tokens; avoid resolving themes inside views or theme callbacks.

## Validation and performance evidence

Run `mise run check` before each commit and `mise run check-repo` for repository or documentation changes. Run `mise run release` for application source or dependency changes. Add meaningful regression coverage for changes to editing, source mappings, layout, file handling, or theme validation. Prefer invariant and differential tests over tests that reproduce implementation steps. Mechanical moves and documentation changes can use existing coverage.

For performance changes, run the relevant release benchmarks and the large-document profile. For example:

```sh
DAYTRIP_PROFILE_BLOCKS=10000 DAYTRIP_PROFILE_SAMPLES=60 mise run profile
DAYTRIP_PROFILE_BLOCKS=50000 DAYTRIP_PROFILE_SAMPLES=30 mise run profile
```

Use the same fixture, toolchain, build settings, and sample count when comparing changes. Record hardware/platform, cold load separately from warm interactions, median/tail values, and the measured phases. Distinguish CPU/software rendering from OS/GPU presentation. Keep timing observations out of ordinary regression assertions. Retain raw evidence and explain interruptions or exclusions; see [the current baseline](docs/performance.md).

Check cursor movement, forward/reverse selection across rich blocks, disclosure, undo, and scrolling when an optimization affects those paths. Preserve exact geometry and source mappings or cover the new behavior explicitly. Native IME, accessibility, and platform behavior still need manual testing beyond the automated harness.

## Public contribution workflow

Open an issue to discuss substantial behavior or architecture changes before
implementing them. Bug reports should include a minimal synthetic reproduction;
use private reporting for security issues. Follow the
[community conduct guidelines](CODE_OF_CONDUCT.md).

Fork [the repository](https://github.com/MoonlarkStudios/daytrip-editor), create a
focused branch, and open a pull request against `main`. Run the applicable checks
above and fill out the PR template. Use the commit-message format below for the PR
title: squash merging uses that title as the public commit subject.

`main` requires a pull request, up-to-date passing Rust checks on macOS, Linux and
Windows, passing Repository hygiene and Pull request title checks, and resolved
review conversations. Force pushes and deletion are disabled, and these protections
include administrators. The current
single-maintainer setup does not require an additional approving reviewer for the
maintainer's own changes; the maintainer reviews outside contributions. Increase the
required approval count when another maintainer is available. Merged branches are
deleted automatically. First-time contributors may need a maintainer to approve
their CI run; workflows receive a read-only token and no persisted Git credentials.

## Commit messages

Each commit represents one logical change and passes the existing mise checks. Keep mechanical organization separate from behavior changes, dependency updates, and convention changes. Commit application `Cargo.lock`; keep generated `target/` output out of Git.

Use a single-line message:

```text
<type>(<optional scope>): <imperative summary>
```

Examples:

```text
feat(editor): reveal syntax around the active inline span
fix(document): preserve reverse selection through disclosure
refactor: organize source modules by responsibility
docs: define Rust and contribution conventions
```

Use an appropriate type such as `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`, or `ci`. Keep summaries concrete. Review the staged diff and `git diff --cached --check` before committing. Push only when requested.

## Documentation and repository maintenance

Keep the [documentation index](docs/README.md), README, bundled guide and architecture map consistent with user-visible changes. Label historical measurements clearly and retain their evidence and limitations. Do not delete a benchmark, fixture, generated brand asset or upstream file merely because application code does not import it; check its documented purpose. Remove unused application code and dependencies instead of silencing warnings. Keep third-party lint exceptions confined to the documented vendor manifest.

CI runs the locked Rust checks and release build on macOS, Linux and Windows, plus repository hygiene, full-history secret scanning and dependency auditing on Linux. GitHub Actions use full commit IDs, read-only or disabled token permissions, no persisted checkout credentials and bounded job durations. Follow [GitHub's workflow security guidance](https://docs.github.com/en/actions/reference/security/secure-use) when changing them. Dependabot proposes weekly compatible Cargo and action updates; review changes and validation before merging. Yrs, cryoglyph and Iced highlighter upgrades follow the explicit vendor rebase policy.

New commits cancel superseded PR build runs. Running `main` builds finish so they
can publish caches; GitHub retains only the latest pending run for that branch.
Title and description edits run a separate title check with no checkout or token
permissions, leaving the build matrix alone. Keep that check required alongside
the four existing checks when changing branch protection.

Only `main` writes the Cargo and mise caches. The repository job has a separate
mise cache namespace and saves after its tasks finish, retaining installed tools
such as cargo-audit and cargo-shear. Tool-version changes in `mise.toml` invalidate
that cache; audits still refresh advisory data on every run. A new cache namespace
is populated by a `main` run after merge, then restored by PRs.

The public repository enables private vulnerability reporting, secret scanning/push protection and Dependabot alerts/security updates. Preserve these hosting settings alongside the branch protections above; checked-in files alone cannot activate them. Review commit author identities and retained logs before publishing; do not rewrite published history or remove evidence casually. Publish the intended branch explicitly rather than pushing every local ref.

## Licensing

Daytrip is licensed under the GNU General Public License, version 3 or (at your
option) any later version (`GPL-3.0-or-later`); see [LICENSE](LICENSE).
Contributions to Daytrip are accepted under these terms. Vendored dependencies
and bundled fonts retain their own licenses and notices.
