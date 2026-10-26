# Koan

[![Check](https://github.com/MoonlarkStudios/daytrip-editor/actions/workflows/ci.yml/badge.svg)](https://github.com/MoonlarkStudios/daytrip-editor/actions/workflows/ci.yml)

A quiet, native Markdown editor built with Rust and Iced. Write with live formatting,
tables, images, and themes, or switch to the Markdown source at any time.

Koan is in early development. See the [current limitations](docs/evaluation.md).
Previously named Daytrip; [existing settings and drafts remain compatible](docs/identity-migration.md).

## Run

Install [mise](https://mise.jdx.dev/getting-started.html) and the
[native build tools](CONTRIBUTING.md#development-environment) for your platform, then:

```sh
git clone https://github.com/MoonlarkStudios/daytrip-editor.git
cd daytrip-editor
mise trust
mise install
mise run dev
```

## Contribute

Read [CONTRIBUTING.md](CONTRIBUTING.md) for setup, checks, and pull requests.
The [documentation](docs/README.md) covers editing, architecture, and design.
Report bugs and suggest features through [GitHub issues](https://github.com/MoonlarkStudios/daytrip-editor/issues).
Report vulnerabilities [privately](SECURITY.md).

Copyright © 2026 Koan contributors. Licensed under [GPLv3 or later](LICENSE).
Bundled fonts use the [SIL Open Font License](assets/fonts/OFL.txt).
