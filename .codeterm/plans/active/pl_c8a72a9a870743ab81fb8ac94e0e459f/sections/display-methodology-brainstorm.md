---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_c3f08b6fd6c84dbca6377ad94cbef58e
handle: display-methodology-brainstorm
title: Display Methodology Brainstorm
---

**HOW the design was sourced — and how that process is itself dogfooded.** The display methodology wasn't designed by one author. Two independent agents on *different providers* (codex + cursor) each proposed how to show plan/spec/tasks on a pan/zoom board; their proposals were synthesized into the v3 design that the substrate sections implement. This is process, not a code module — but the implementation choices below are the concrete residue of that brainstorm, each traceable to a real file:

- **"semantic zoom that never hides info"** → `worlds.ts` (per-node apparent-width LOD bands W0–W3) + the crisp-raster trick, so detail *adds* on approach and never blanks out.
- **"bounding the портянки" (no kilometer-tall nodes)** → `taskFootprint.ts` two-axis bounded sizing (`clamp` on volume, `taskMaxW`/`taskMaxH`) + reading-band internal scroll instead of unbounded growth.
- **"plan + spec are one node type"** → `nodeRegistry.tsx` kind→renderer dispatch reusing `RichMarkdown`, not a reduced variant.
- **"mermaid / code at scale"** → the same `@codeterm/chat-ui` `RichMarkdown` renderer the chat view uses, so diagrams/code render identically on the board.
- **"auto-bounds + test layout integrity"** → `layout/regionPacking.ts` deterministic packing + the Playwright bounding-box suite (`tests/playwright/suites/13-canvas`).

The brainstorm artifacts themselves live *in the plan* as provenance `.md` attached to tasks (see *Provenance Discussions Captured In The Plan*) — so the design discussion is connected to the work it shaped, and reviewing "why is it like this" is one zoom away on the board.

## Key decisions
- **Cross-provider divergence on purpose** — two providers reason differently; synthesizing two independent designs caught blind spots a single author (or single model family) would share.
- **Capture the process as data** — the decision trail is stored through the system being built, which is the dogfooding thesis of this whole plan.
