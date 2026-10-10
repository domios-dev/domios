# Domios logo concepts

**Latest: [round three — Threshold refinements and Liquid motion](round-3/README.md).** The owner selected original 03 as the direction to develop. [Round two](round-2/README.md) and the first-round proposals below remain available for comparison.

Four directions for **the OS for AI agents**. Tagline: **Agents live here.**
These are proposals, with no winner selected and no changes to the live site's header or favicon.

Start with [the overview](overview.png), then open the individual sheets at 100% zoom to judge the actual 16 px and 32 px samples. Each sheet includes a 512 px mark, both theme palettes, monochrome, wordmark context, and browser tabs with true 16 px favicon images.

## 01 — Hearth

A pitched roof flows into a rounded D shoulder. The broad open doorway makes a welcoming home; the asymmetric silhouette gives it character.
Its single connected contour keeps the roof and entrance legible when reduced.

[Concept sheet](01-hearth-sheet.png) · [SVG](01-hearth/color.svg)

## 02 — Resident

A rounded presence with a single window and a jaunty folded hem. Part spirit, part tiny dwelling: playful without a robot face or fine details.
The square opening is deliberately large, and the stepped hem adds personality without eyes or decorative strokes.

[Concept sheet](02-resident-sheet.png) · [SVG](02-resident/color.svg)

## 03 — Threshold

A sturdy D opens at its lower corner, turning its counter into an entrance. The diagonal cut suggests stepping inside: a compact symbol for an agent OS.
This direction puts the initial first, with the doorway as a second reading.

[Concept sheet](03-threshold-sheet.png) · [SVG](03-threshold/color.svg)

## 04 — Commons

Two rooftops share a single courtyard, turning a house into a small community. The broad, twin-peaked silhouette suggests agents living and working together.
One large negative square carries the interior; there are no tiny nodes or connecting lines.

[Concept sheet](04-commons-sheet.png) · [SVG](04-commons/color.svg)

## Assets and palette

Every concept uses a 64 × 64 unit canvas and one filled mark path. Negative space is transparent; there are no strokes, gradients, masks, external resources, or font dependencies in the mark SVGs.

| Asset | Purpose |
| --- | --- |
| `color.svg` | Transparent brand blue `#58a6ff`, for dark surfaces |
| `color-light.svg` | Transparent deep blue `#0969da`, for light surfaces |
| `mono.svg` | One ink using `currentColor`; defaults to black as a standalone image |
| `mono-white.svg` | White knockout on transparency |
| `on-dark.svg` | Brand blue on the site's `#0d1117` background |
| `on-light.svg` | Deep blue on the site's `#f6f8fa` background |
| `dark-{512,64,32,16}.png` | Transparent blue raster samples for dark backgrounds |
| `light-{512,64,32,16}.png` | Transparent blue raster samples for light backgrounds |

Both blue values and the background colors come from `web/src/tokens.css`. The wordmark on the sheets is a contextual Segoe UI Bold typesetting of `domios`, not a proposed final bespoke wordmark.

## Reproducing the sheets

From `web/`, run `python brand/render-concepts.py`. It needs Pillow, installed Segoe UI / Consolas fonts, and headless Microsoft Edge. No web server or project build is involved. For other environments, `--browser` accepts a Chromium-compatible executable and `--font-dir` accepts a directory containing the same font filenames; fonts are not bundled here.

The script renders all marks at 512 px in a single transparent browser atlas, then uses Pillow's Lanczos resampling for the 64, 32 and 16 px images. Those exact rasters appear in the sheets and mock tabs without any per-size geometry edits. Temporary browser files stay in the ignored `web/brand/.render/` folder. Add `--round 2` to render the latest concepts; the default preserves round one.

The first rendering attempt found the Python CairoSVG package but no native Cairo runtime on this Windows machine. The generator therefore uses the installed headless browser, avoiding an additional native dependency.

## After selection

The chosen direction will receive the final logo, monochrome mark, outlined wordmark lockup, SVG/ICO favicon, 180 px Apple icon, 192/512 px icons, and 1200 × 630 social image. Header and metadata integration belongs to that later selection step.
