# Domios / Threshold

The owner selected the **original 03 Threshold**, unchanged. Its room opens into a passage: a place to inhabit, with a way for work to leave for the next machine. The vector is a cut in solid material, not a diagram of computers. That physical doorway supplies both the silhouette and the motion. One broad shape carries the identity at 16 px.

## Assets

Every delivery asset in this folder is copied to `web/public/` at the same filename.

| Asset | Use |
| --- | --- |
| `logo.svg`, `logo-mono.svg`, `logo-white.svg` | Blue, inherited `currentColor`, white |
| `wordmark-horizontal.svg`, `wordmark-compact.svg` | Outlined lowercase domios lockups; no font dependency |
| `favicon.svg` | Light/dark browser preference, accessible blue on each background |
| `favicon.ico` | 16, 32 and 48 px fallback |
| `apple-touch-icon.png` | 180 px |
| `icon-192.png`, `icon-512.png` | Application icons |
| `maskable-512.png` | Full bleed background; mark inside the central safe circle |
| `site.webmanifest` | Standalone identity and icon declarations |
| `og-image.png`, `og-image.svg` | 1200 × 630 social image and outlined vector source |

The lockup lettering is outlined from Segoe UI Bold. All exports use the original path in `brand/concepts/03-threshold/color.svg`. On Windows, regenerate with `python brand/final/build-assets.py` from `web/`; the generator uses installed Segoe UI fonts, FontTools, Pillow and an isolated headless Edge profile. The browser-facing SVG favicon follows the browser's theme; inline marks follow the site's selected theme.

## Site motion

`src/BrandMark.tsx` owns every inline mark. Its default is static, including the hero eyebrow and machine badges. `motion="load"` plays Surface tension once at mount and on later hover/focus. `motion="interactive"` plays on hover/focus only. Both opt-in modes support A little give on pointer or Space/Enter press and hold, followed by release. Focus and native Enter activation remain on the parent link. Static marks add no listeners or animation frames.

The surface opens rapidly, overshoots and settles. Press compression uses `--ct-ease-liquid-settle`; release uses the same analytic spring as the approved prototype: mass 1, stiffness 420, damping 22. Every animation returns to the exact original path, with no group transform left behind. Fixed SVG dimensions prevent layout shift. Reduced motion, hidden documents and the ancestor `data-motion="paused"` reset the mark immediately. No breathing or arrival animation runs on the site.
