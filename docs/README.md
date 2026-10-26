# Documentation

Start with the [project README](../README.md) to build and use Daytrip, and
[CONTRIBUTING.md](../CONTRIBUTING.md) to change it. The application is a working
technology evaluation; [release gaps](evaluation.md#remaining-release-work)
describe the remaining platform, accessibility, and durability work.

## Current behavior and design

| Document | Scope |
| --- | --- |
| [Architecture](architecture.md) | Source boundaries, editing invariants, layout, themes and storage |
| [Writing workflows](writing-workflows.md) | Links, images, credits, assets, recovery and HTML export |
| [Collaboration](collaboration.md) | CRDT transactions, selections, offline replicas, history and persistence |
| [Outline](outline.md) | Heading search, collapse state and section moves |
| [Code editing](code-editing.md) | Fences, indentation, clipboard behavior and bounded highlighting |
| [Appearance](design-system.md) | Semantic tokens, typography, presets and UI validation |
| [Brand kit](../branding/daytrip/README.md) | Asset sources, generated exports and platform icons |
| [Yrs pending-update repair](yrs-pending-updates.md) | Sparse-clock retry bug, regression and repeated upstream tests |
| [Dependency health](dependency-health.md) | Dependency repairs, enforced maintenance exceptions and validation |

Update behavior descriptions alongside code. Keep dated validation sections tied
to the implementation they measured; old test counts are historical observations.

## Measurement history

These reports explain successive implementations and preserve their raw evidence.
They are not interchangeable current benchmarks. [History references](history.md)
map commit IDs retained in raw logs to the public history. Reproduction commands, fixtures,
hardware, sample counts and exclusions live with each report.

1. [Original large-document baseline](performance.md)
2. [Viewport virtualization](virtualization.md)
3. [Deferred native-buffer initialization](native-buffer.md)
4. [Incremental block parsing](incremental-parsing.md)
5. [Native text snapshots](native-snapshots.md)
6. [Source and projection updates](source-projection-updates.md)
7. [CRDT integration](crdt-performance.md)
8. [Cached UTF-16 traversal](utf16-performance.md)
9. [Incremental offline imports](offline-imports.md)
10. [Writing workflows and retention](writing-workflows.md#reproducible-performance)
11. [Code highlighting](code-editing.md#measurements)
12. [Outline operations](outline.md#release-measurements)

[Native macOS validation](native-writing-validation.md) distinguishes observed
clipboard/drop/input behavior from model tests and records outstanding platform
checks. Raw logs, source hashes and screenshots remain beside their reports.
