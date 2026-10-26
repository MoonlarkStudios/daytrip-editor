# Repository instructions

Read [CONTRIBUTING.md](CONTRIBUTING.md) before changing this repository. It defines the source boundaries, applicable Google Rust guidance and project exceptions, validation, and commit policy.

- Keep one Cargo package. Group work under `src/{app,document,editor,theme,io}`; keep `main.rs` thin and public exports deliberate.
- Use the mise-pinned Rust toolchain. Run `mise run check` before each commit and `mise run release` for application source changes. Performance changes also need relevant release benchmarks and large-document profiles.
- Run `mise run check-repo` for repository/documentation changes and `mise run audit` for dependency changes. Keep current behavior docs distinct from historical validation evidence; review [dependency health](docs/dependency-health.md) before changing vendored code or audit policy.
- Test private implementations beside their modules; shared inputs belong in `tests/fixtures`. Do not expose internals solely for tests.
- Preserve authoritative Markdown, UTF-8 source selections with anchor/head direction, lossless source/display mappings, undo, and continuous movement across rich blocks.
- Contextual disclosure must preserve source caret/selection, wrap affinity, and viewport anchoring through local reflow. Hidden syntax occupies no reserved space.
- Resolve themes on load/switch, store `ResolvedTheme` in `App`, and return its stored Iced theme from the theme callback. Pass the same semantic tokens to centralized styles and drawing.
- Keep blocking IO and image decoding off the UI event path. Preserve existing file guards and resource limits.
- Prefer clear ownership and small cohesive changes. Unsafe code is forbidden. Do not introduce abstractions or additional crates without a concrete need.
- Keep module moves separate from behavior changes. Use single-line `<type>(<optional scope>): <imperative summary>` commit messages; commit `Cargo.lock`, exclude generated build output, and push only when requested.
- Preserve user changes, existing windows, and unsaved documents during manual validation.

Consult [docs/architecture.md](docs/architecture.md) and [docs/performance.md](docs/performance.md) before changes to editing or layout. Record reproducible measurements and their limitations; do not add machine-dependent timing assertions to ordinary tests.
