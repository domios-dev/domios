# Domios — round two

**The distributed OS for AI agents. Agents live here.**

[Combined overview](overview.png). Individual sheets include the mark at 512 px, actual 64/32/16 px downscales on both themes, monochrome, wordmark context and mock browser tabs. Open at 100% zoom to judge favicon pixels. These are four proposals; the site remains unchanged pending selection.

## Design vector

The starting words were **fit, share, hold, mend**.
They led to a carpenter's joint, flatmates under a sheet, a suspended sling, and stretched cloth.
Distribution lives in the relationship between separate parts and a shared space; the marks use no initials.
The large shapes and physical metaphors give the system a domestic, slightly mischievous character.

## 05 — Joinery

A fitted wooden joint, reduced to two chunky pieces and one generous seam. Machines stay independent; Domios is the fit that lets them work as one.
The two pieces have different proportions, with an 8-unit clearance that stays open at favicon size.

[Sheet](05-joinery-sheet.png) · [SVG](05-joinery/color.svg)

## 06 — Tandem

Two mischievous flatmates under one sheet: unequal heights, offset windows, one crooked hem. The paired residents give the distributed system a social presence.
Two 12-unit windows remain visible without the fine features of a face; the windows can also read as rooms on different hosts.

[Sheet](06-tandem-sheet.png) · [SVG](06-tandem/color.svg)

## 07 — Sling

A small sling stretched between two anchors. Its rounded ends and deep belly turn a connection into somewhere to settle: different hosts, shared support.
The connection itself forms the dwelling, and the single contour stays legible without an interior detail.

[Sheet](07-sling-sheet.png) · [SVG](07-sling/color.svg)

## 08 — Patch

A fabric patch pulled from four corners, keeping one big window open between them. Mesh becomes a shared material: soft, useful and held together by peers.
The four corners gather around a 16-unit square opening, joining a tactile silhouette with a domestic window.

[Sheet](08-patch-sheet.png) · [SVG](08-patch/color.svg)

## Files and reproduction

Every folder contains the same six SVG variants as [round one](../README.md#assets-and-palette): `color`, `color-light`, `mono`, `mono-white`, `on-dark`, and `on-light`. Transparent PNGs at 512/64/32/16 px are provided for both palettes. Site colors remain `#58a6ff` / `#0969da`, on `#0d1117` / `#f6f8fa`.

The SVGs have a 64-unit viewBox, one filled path, transparent negative space and accessible title/description. They use no strokes, gradients, masks, fonts, external resources or animation. The sheet wordmark is contextual Segoe UI Bold, pending final lockup design.

From `web/`, run `python brand/render-concepts.py --round 2`. Geometry and rationale source: [concepts.json](concepts.json). The renderer uses headless Edge plus Pillow; `--browser` and `--font-dir` override local executable/font locations. All smaller PNGs are Lanczos reductions of the same 512 px render, without per-size adjustments. The first round's assets are preserved.

These are static studies. Any later animated application should use the existing `ct-ease-liquid-*` tokens and `.ct-liquid-*` controls. No alternative motion system is introduced here.

The final asset set and site integration follow the owner's selection.
