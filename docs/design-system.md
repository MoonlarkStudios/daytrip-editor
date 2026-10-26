# Daytrip appearance

Daytrip's open book and outlined lowercase signature remain the identity. The application extends the brand into a quiet writing environment: warm surfaces, confident ink, restrained accents, and generous space around the document. The logo has no tagline.

## Atmospheres

| Preset | Canvas and chrome | Ink and emphasis |
| --- | --- | --- |
| Paper | Warm paper, limestone sidebar | Plum ink, mauve book leaf, deeper plum controls |
| Midnight | Warm charcoal and plum surfaces | Linen text, soft lilac controls |
| Sage | Linen paper, pale botanical chrome | Dark green ink, leaf accents |

The approved brand palette remains in [the brand kit](../branding/daytrip/README.md). Decorative mauve is separate from the stronger interactive accent: a small label or link must remain readable. Each preset has its own semantic colors in `themes/`, rather than a tint over one universal palette.

Newsreader supplies document text and editorial headings. System sans serif supplies controls. Monospace supplies source, code, and paths. The sidebar signature uses the actual outlined wordmark, never a font approximation. Its Iced paths are generated from the SVG masters by the brand exporter; no SVG parsing, image decoding, or font lookup happens to render the signature.

## Roles and state

`ResolvedTheme` owns the semantic tokens and Iced palette. Presets and previews resolve at startup; custom themes resolve when loaded. Views consume the stored result.

- `canvas`, `surface`, and `elevated` distinguish the document, navigation, and dialogs.
- `text`, `muted`, and `faint` establish hierarchy while retaining readable small labels.
- `accent` identifies actions; `accent_soft` identifies the current selection. `selection` is reserved for selected document text.
- `brand_accent` colors the decorative book leaf. It is optional in custom themes and falls back to `accent`.
- Primary and selected foregrounds, hover surfaces, control borders, disabled colors, scrims, and shadows derive once during resolution. Primary foregrounds accommodate both dark and pale custom accents.
- Hover and pressed states provide feedback. Disabled undo/redo controls are visibly unavailable; the current file retains its selected treatment even without an action.
- Inputs use a visible boundary and a stronger focus outline. Scrollbars, notifications, code surfaces, and dialogs use the same palette as the document.

Built-in theme tests check normal text and interactive accent contrast against their intended surfaces at **4.5:1**, selection readability, and input boundaries at **3:1**. These thresholds follow [WCAG contrast guidance](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum); they are color checks, not an accessibility certification. Custom files retain their existing colors and validation behavior; arbitrary custom palettes are not automatically contrast-corrected.

## Layout and behavior

The sidebar keeps the signature, Files/Outline navigation, and document actions together. The document remains the central surface. Formatting uses a consistent vector icon set with tooltips. Focus mode hides the sidebar and formatting toolbar while retaining the document title, exit control, and writing status.

The status bar reduces secondary detail in narrow windows. Dialogs remain within the viewport; appearance settings keep their header and footer available while their contents scroll. Preset previews display each palette and identify the current selection. Text-size controls use the existing session zoom behavior and keyboard shortcuts.

HTML exports use Paper's resolved palette for a consistent, portable reading page. Raw HTML escaping, content security policy, embedded assets, and print behavior remain intact. Exports add no logo or promotional footer to the writer's document.

## Verification

```sh
mise run check
mise run release
python3 branding/daytrip/export.py --check
DAYTRIP_CONFIG_DIR=/tmp/daytrip-theme-preview mise exec -- cargo test --locked theme_ui_previews -- --ignored --nocapture
```

The private preview test renders the real application through Iced's software renderer at 1180×820 and the minimum 760×520 window size. It covers all presets, appearance settings, focus mode, find/replace, long titles, source and reading modes, code blocks, and the unsaved dialog. PNGs are written to `target/theme-validation/`. It does not open a native window or modify the user's preferences. Theme-switch regression coverage preserves document source, reverse selection, and undo availability. Native OS presentation and assistive technology behavior require separate platform validation.

Validated on 2026-10-25 with the mise-pinned Rust 1.95.0 and Iced 0.14: the isolated theming-only snapshot passed 160 regression tests (12 opt-in tests ignored), and the full development checkout passed 184 tests (16 ignored); formatting and Clippy passed; release and the signed macOS bundle built successfully; all 41 derived brand assets reproduced. The 21 software previews were visually inspected. A separate native macOS session with isolated preferences confirmed the outlined signature, theme switching, settings scrolling, text-size control, and focus mode. The native check found scrollbar overlap in the shortcut list; the final layout reserves space beside that rail. The validation window was closed without editing or saving a document.
