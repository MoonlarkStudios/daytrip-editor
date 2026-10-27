# Releases

Dosty uses manually chosen versions in the root `Cargo.toml`. The GitHub
[Release workflow](../.github/workflows/release.yml) builds native packages and
creates **drafts only**. It does not publish releases, push tags, sign with a
trusted macOS/Windows identity, or upload packages to distribution repositories.

## Prepare a version

Use Python 3.11 or newer for release scripts. Through the pinned mise environment:

```sh
mise run prepare-release -- 0.1.0-alpha.2
```

This edits the package version and updates only the workspace package in
`Cargo.lock`, offline. Review both files. Do not update third-party dependencies
as part of an ordinary version bump. Write `docs/releases/0.1.0-alpha.2.md` with
changes, installation instructions and known limitations; that file becomes the
GitHub release description. Run the applicable contribution checks and merge
through the normal PR process.

Versions have the form `MAJOR.MINOR.PATCH`, optionally followed by `-alpha.N`,
`-beta.N` or `-rc.N` with a positive counter. During early development, advance
the alpha counter for testing releases, patch for fixes and minor for feature
releases. Native numeric fields limit each main component to 65535; build
metadata and arbitrary prerelease identifiers are intentionally unsupported.

All platform packages share that application version:

| Field | Example |
| --- | --- |
| Cargo version and download names | `0.1.0-alpha.1` |
| Git tag | `v0.1.0-alpha.1` |
| macOS short version | `0.1.0` |
| macOS full `DostyVersion` field | `0.1.0-alpha.1` |
| Windows installer numeric version | `0.1.0.BUILD` |
| Debian package version | `0.1.0~alpha.1-1` |
| RPM version / release | `0.1.0~alpha.1` / `1` |

`DOSTY_BUILD_NUMBER` supplies BUILD and macOS's separate numeric bundle build
version. CI uses `github.run_number` (reruns retain their number); local builds
default to 1. Values 1–9999 are accepted. The full prerelease identifier stays in
the package names, notes and platform metadata. Linux's `~` makes alphas, betas
and release candidates sort before the final version. Native versions are
generated; never update the platform templates for a release bump.

Download filenames always use the Cargo version, including its hyphenated
prerelease suffix: `dosty_0.1.0-alpha.1-1_amd64.deb` and
`dosty-0.1.0-alpha.1-1.x86_64.rpm`. The internal package versions still use `~`.
This keeps download names stable when uploaded: [GitHub can rename special
characters in asset filenames](https://docs.github.com/en/rest/releases/assets#upload-a-release-asset).
The packager and checksum writer use the same filename helper.

## Create a draft

After the version commit is on `main`, a maintainer creates and pushes only its
intended tag. For the initial release:

```sh
git tag -a v0.1.0-alpha.1 COMMIT_ON_MAIN -m "Dosty 0.1.0-alpha.1"
git push origin refs/tags/v0.1.0-alpha.1
```

Replace `COMMIT_ON_MAIN` with the reviewed commit containing that version. The
tag push starts the workflow. Alternatively, use **Actions → Release → Run
workflow**, selecting `main` and providing an existing matching tag. It never
creates a missing tag. Tags must match Cargo and the lockfile, point to the
checked-out commit, and be ancestors of `origin/main`.

The workflow runs repository, audit, vendored Yrs and secret checks, then builds
and tests the tagged commit on each platform using the mise-pinned Rust toolchain
and locked dependencies. Only the final draft job has repository write
permission. Actions are pinned to commit IDs, checkout credentials are not
persisted, and release jobs do not save shared caches. Packaging changes also
build on pull requests, without creating a release or using a write token.

| Artifact | Build baseline |
| --- | --- |
| `Dosty-VERSION-macos-arm64.dmg` | macOS 15 runner, macOS 12 deployment target |
| `Dosty-VERSION-macos-x86_64.dmg` | macOS 15 Intel runner, macOS 12 deployment target |
| `Dosty-VERSION-windows-x86_64-setup.exe` | Windows Server 2025, x86_64 MSVC, statically linked CRT |
| `dosty_VERSION-1_amd64.deb` | Ubuntu 24.04, dynamically detected ELF dependencies plus desktop runtime libraries |
| `dosty-VERSION-1.x86_64.rpm` | Fedora 43 container, automatic ELF dependencies plus desktop runtime libraries |
| `dosty-VERSION-source.tar.gz` | Exact tagged Git tree, including vendored patches, Cargo.lock and build scripts |
| `SHA256SUMS` | SHA-256 hashes of the six artifacts above |

macOS images contain an ad-hoc-signed `Dosty.app` and Applications shortcut.
The Windows installer installs per user, adds a Start menu shortcut, and registers
an uninstaller without requiring administrator access. It does not launch or
close Dosty during installation. Packages include the application license,
font notices and third-party notices generated once on Linux with pinned
`cargo-about`, including local vendored dependencies. Every native package uses
that same report covering all release targets. The tool installs from pinned
Cargo source so local Intel Mac packaging does not require an upstream prebuilt
binary. This is release tooling, not an additional application dependency.
Missing/unidentified licenses fail generation.

Linux packages install the application, desktop launcher, icon and licenses.
Dependencies include X11/Wayland runtime libraries, D-Bus, `xdg-utils`, the
desktop portal and Zenity fallback. A working desktop portal backend is still
needed for native portal dialogs; a GTK backend is recommended. The DEB baseline
targets Ubuntu 24.04+ and Debian 13; RPM targets Fedora 43+. These are initial
validation targets, not claims of complete desktop compatibility. RPM support
does not imply RHEL, openSUSE or other distributions are supported.

The final job requires all five binary packages plus matching source before
creating the draft. Prerelease versions also set GitHub's prerelease flag.
After upload, the workflow reads the draft's metadata through its API URL and
verifies that its names, sizes and GitHub SHA-256 digests match all seven local
files, including `SHA256SUMS`. Renamed, missing, extra or incomplete assets fail
the job; failures leave the release unpublished for inspection. The release must
also retain the expected tag, draft and prerelease flags.
The workflow refuses an existing release instead of replacing its assets. If a
failed upload leaves a partial draft, inspect and delete that **draft** manually
before rerunning. Never delete a published release or move a released tag to
reuse its version; use a new version for fixes and packaging corrections.

## Verify the draft before publishing

Automation verifies macOS bundle signatures and disk images, Windows silent
installation/uninstallation, and Linux package installation, isolated startup
under Xvfb/software rendering, and removal. These checks do not replace native
desktop testing, notarization or trusted signing.

Download the actual draft assets through a browser and verify their hashes.
Test installation and startup on each advertised OS/architecture, then opening,
editing, Unicode/IME input, clipboard, saving, undo and recovery using synthetic
documents and an isolated `DOSTY_CONFIG_DIR`. Preserve existing windows and
unsaved work. Record exact OS versions and results in the release notes before
manually publishing. Follow the [remaining release work](evaluation.md#remaining-release-work),
especially accessibility and native input limitations.

For local packaging, first run `mise run release-notices`, then use the appropriate
native host with `mise exec -- bash scripts/package-macos.sh`,
`mise exec -- pwsh -File scripts/package-windows.ps1`, or (after
`mise run release`) `python3 scripts/package-linux.py deb` / `rpm`. Windows needs
Inno Setup 6; Linux needs the native package tools and `desktop-file-validate`.
Outputs stay under ignored `target/`. `scripts/smoke-linux.sh` is intended only
for disposable CI desktops, never an existing user session.
