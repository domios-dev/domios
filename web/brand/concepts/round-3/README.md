# Threshold — round three

The owner selected **03 Threshold** as the direction to develop. This round compares the unchanged original with six refinements, keeping the open lower corner and a single connected silhouette.

[Comparison overview](overview.png) · [Logo Lab data](logos-round3.json) · [Interactive motion prototype](../../motion/index.html)

| ID | Refinement | What changes | Full sheet |
| --- | --- | --- | --- |
| 03 | Original | Unchanged comparison baseline | [Sheet](03-original-sheet.png) |
| 03a | Welcome | A 20-unit room and wider 45-degree entry | [Sheet](03a-welcome-sheet.png) |
| 03b | Softneck | A curved passage, pulled open like a liquid neck | [Sheet](03b-softneck-sheet.png) |
| 03c | Peek | An attached resident emerging from the left jamb | [Sheet](03c-peek-sheet.png) |
| 03d | Roundhouse | A fuller bowl, rounded shoulder and inner corners | [Sheet](03d-roundhouse-sheet.png) |
| 03e | Swing | A broad sloping plane suggesting a door swung outward | [Sheet](03e-swing-sheet.png) |
| 03f | Bold | A larger 56-unit footprint and heavier walls | [Sheet](03f-bold-sheet.png) |

Each full sheet shows a 512 px mark, actual 64/32/16 px downscales on both themes, wordmark context, monochrome, and dark/light browser tabs with 16 px favicons. Open at 100% to judge the small sizes. The comparison overview uses larger labels and shorter descriptions for scanning.

Each concept folder includes six SVG variants (`color`, `color-light`, `mono`, `mono-white`, `on-dark`, `on-light`) and transparent PNGs at 512/64/32/16 px in both blue palettes. All marks use the same 64-unit viewBox and one filled path. Smaller images are genuine Lanczos reductions of their 512 px masters, with no per-size drawing changes.

`logos-round3.json` is an array of exactly six refinements, each with `{id, name, desc, d, rule}`. `id` is a string from `03a` through `03f`; `rule` is `evenodd`. Render `d` into a `viewBox="0 0 64 64"` path with the supplied fill rule. The original 03 is intentionally excluded from this import list to avoid duplicating its existing Logo Lab entry.

From `web/`, reproduce with `python brand/render-round3.py`. It uses installed headless Edge and Pillow, plus the presentation helpers in `render-concepts.py`. `--browser` and `--font-dir` override local executable/font locations. Font and palette choices match the preceding rounds. The source SVG for original 03 is read directly, rather than reconstructed.

Motion is studied on the original 03 to keep the animation decision separate from the six shape choices. See the [motion notes](../../motion/README.md). The site header and favicon remain unchanged until the final selection.
