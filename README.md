# Dosty

[![Check](https://github.com/MoonlarkStudios/daytrip-editor/actions/workflows/ci.yml/badge.svg)](https://github.com/MoonlarkStudios/daytrip-editor/actions/workflows/ci.yml)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="branding/dosty/logos/pen-page-reverse.svg">
  <source media="(prefers-color-scheme: light)" srcset="branding/dosty/logos/pen-page.svg">
  <img src="branding/dosty/logos/pen-page.svg" alt="Dosty inkwell logo" width="96" height="96">
</picture>

A quiet, native Markdown editor built with Rust and Iced. Write with live formatting,
tables, images, and themes, or switch to the Markdown source at any time.

Dosty is in early development. See the [current limitations](docs/evaluation.md).
Previously named Koan and Daytrip; [existing settings and drafts remain compatible](docs/identity-migration.md).

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

Copyright © 2026 Dosty contributors. Licensed under [GPLv3 or later](LICENSE).
Document fonts use the [SIL Open Font License](assets/fonts/OFL.txt). The
[DM Sans brand source](branding/dosty/sources/DMSans-OFL.txt) has its own OFL notice.
