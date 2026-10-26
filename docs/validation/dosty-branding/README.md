# Dosty branding validation · 2026-10-26

The initial rebrand was validated on macOS/Apple silicon with mise-pinned
Rust 1.95.0, Iced 0.14, Python 3.13, and ImageMagick 7.1.2-31. It used a
Fraunces wordmark with the selected smoked teal pen/page mark. The DM Sans
follow-up below replaces that wordmark and corrects its native contour filling.

## Form fidelity

The canonical SVG was traced from the approved mockup crop, rather than redrawn
from an approximate letter. The original aspect ratio, curl, slanted ascender,
negative-space pen, breather, and slit are retained. Three flat color regions
replace the raster texture. The compact master retains the outer silhouette and
strengthens the internal pen details.

[mark-comparison.png](mark-comparison.png) shows the approved raster, the vector
rendered back at the same size, and their contour overlay. Shared pixels are
dark; mockup-only pixels are red; vector-only pixels are blue.
[geometry.json](geometry.json) records **99.12% silhouette intersection-over-union**
at 316×378 px. Remaining differences are spline fitting, classification of
antialiased raster edges, and rendering. This is a form comparison, not a claim
that flat vector colors reproduce the original raster texture pixel for pixel.

The reference crop and SHA-256 are retained with the
[masters](../../../branding/dosty/sources). `export.py --check` also checks the
reference checksum and requires at least 99% silhouette overlap, independently
of the byte-for-byte reproducibility of derived assets.

[icon-sizes.png](icon-sizes.png) shows the supplied 16, 24, 32, 48, and 64 px
icons at actual size, on cream and charcoal. The fixed launcher palette stays
consistent across application themes.

## Checks

```sh
DOSTY_CONFIG_DIR=/tmp/dosty-brand-check mise run check
mise run check-repo
mise run release
python3 branding/dosty/export.py --check
DOSTY_CONFIG_DIR=/tmp/dosty-theme-preview mise exec -- cargo test --locked theme_ui_previews -- --ignored --nocapture
mise exec -- bash scripts/bundle-macos.sh
codesign --verify --deep --strict --verbose=2 target/Dosty.app
```

- Formatting, strict Clippy, and all **187 tests** passed; 16 opt-in tests ignored.
- Repository links, unused dependencies, workflow syntax, and seven audit-policy
  tests passed. Cargo.lock changes only the application package name, with no
  dependency version or checksum changes.
- The optimized build passed. The ad-hoc-signed bundle passed strict verification;
  its name, display name, executable, `app.dosty.editor` identifier, and exact
  ICNS bytes were checked.
- All 41 derived SVG, PNG, ICO, ICNS, native-path, and preview assets reproduced.
- The software renderer generated 21 application previews in
  `target/theme-validation/`. Paper, Midnight narrow, and Sage were inspected for
  the new signature, layout, text, and theme colors. Existing contrast tests
  passed after adjusting Midnight's selection surface.
- Legacy theme files without the new optional logo roles remain valid. Existing
  checkpoint/lock and recovery tests passed with retained Daytrip filenames.

No native editor was launched, and no existing windows or unsaved documents were
changed. This does not add native accessibility or performance evidence. See
[identity and compatibility](../../identity-migration.md) for environment aliases,
replica extensions, and retained data paths.

## DM Sans wordmark follow-up · 2026-10-26

A subsequent native screenshot exposed cutouts in the original Fraunces
wordmark. The native canvas used even-odd filling while the font/SVG outlines
used nonzero winding; an independent render of the original outline confirmed
that changing this rule affected overlapping glyph contours. The wordmark now
uses DM Sans Semibold at optical size 24, with the font's OpenType kerning shaped
by HarfBuzz. Both SVG and native canvas explicitly use nonzero winding.

The selected smoked teal symbol masters are unchanged. Wordmark SVGs, lockups,
native paths, and the brand sheet were regenerated. The actual sidebar was
checked in the Paper, Midnight, and Sage software-rendered previews. The
[current sidebar comparison](wordmark-sidebar.png) shows the original and
updated wordmarks at the application's actual size. Font provenance and license
are retained in the [brand kit](../../../branding/dosty/README.md).

The follow-up passed all 187 tests, formatting, strict Clippy, repository checks,
41 reproducible asset comparisons, and the release build. The signed macOS
bundle was rebuilt and verified against the new release build UUID.

## Optical alignment follow-up · 2026-10-26

DM Sans Semibold is the selected brand typeface. The horizontal signature now
places the wordmark 16 units below its geometric center within the 256-unit
mark height, equivalent to 2.5 px at the sidebar's 40 px size. This compensates
for the y descender and balances the main letter shapes with the symbol's
heavier lower bowl. The exporter applies the same translation to horizontal
SVG lockups and native paths. Standalone wordmark and symbol masters are
byte-for-byte unchanged.

The [actual-size comparison](wordmark-alignment.png) shows the adjustment in
Paper, Midnight, and Sage, using the application's software renderer.

## Native application preview · 2026-10-26

After the wordmark and alignment were approved, the signed release was opened
as a separate Dosty preview bundle configured with an isolated
`DOSTY_CONFIG_DIR`. The native **Welcome.md — Dosty** window was brought forward
and visually inspected. It displayed the approved smoked teal symbol and
DM Sans wordmark with the optical offset in the sidebar. The existing Koan
window was left open; no user document was opened, edited, or saved. The
preview bundle and its settings remain under ignored `target/` output. This
confirms native presentation of the branding, not accessibility, IME, or
editing-performance coverage.
