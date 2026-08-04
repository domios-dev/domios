# Zoom-drill INTO tasks — brainstorm (§24.4)

<!-- revision: 1 -->

Brainstorm for §24.4: "I want to FALL INTO a task by ZOOM, not be forced to click it."
Honors §22.1 (frame fixed, content adapts), §24.1 (ONE behavior, no Doc/Card/Tile buttons),
§24.2 (scroll = node-only, no hand-off), §24.3 (SPEC|PLAN|tasks), §24.5 (status emphasis),
§16.9 (Thread & Coil steps). DESIGN ONLY.

## 0. The load-bearing decisions (TL;DR)

1. **LOD driver = the task's APPARENT ON-SCREEN SIZE (px), not raw camera scale.**
   `appW = taskCellWidth_world × cameraScale`. A task reveals its interior when it occupies
   enough *screen* to be readable — which is exactly what "falling in" feels like. Raw scale
   would reveal a tiny task's body at the same scale as a giant spec node; apparent-size makes
   every node cross its own bands at the moment it's big enough to read. (Scale equivalents
   given below for a default 280px-wide task so it's concrete and testable.)

2. **RESERVE-AT-DEEPEST packing → overlap is impossible.** The packer sizes every task CELL at
   its deepest-LOD bbox (task body frame + ~6 step cards + subtask super-cell), bounded, with
   internal scroll for the long tail (§24.2). Content-LOD only changes what is PAINTED inside
   that always-reserved box. At far zoom the dot floats in reserved whitespace (sub-pixel air,
   reads fine); at near zoom it fills. No reflow, no jitter, no overlap — worst case was always
   allocated. This is the clean reconciliation of "fixed footprint" (§22.1/§24.4) + "content
   deepens" + "no overlap" (§20.3 sweep).

3. **Two emphasis mechanisms, orthogonal:**
   - **Band** (what detail is painted) = zoom-only, per-node-size. Pan never changes a band.
   - **Dominance focus-assist** (which task you "fell into") = viewport-relative: the task
     filling the viewport center elevates + neighbors soft-dim. This delivers "THIS task,"
     distinct from its banded siblings, without a modal mode.

## 1. The content-LOD ladder for a TASK node (far → near)

Driver `appW` = on-screen width of the task cell in CSS px. Hysteresis ±10% at each boundary
(crossfade zone). Scale column assumes `taskCellWidth_world ≈ 280`.

| Band | appW (px) | ≈ scale | What is PAINTED inside the fixed frame |
|------|-----------|---------|----------------------------------------|
| **L0 GLYPH** (far) | < 150 | < 0.54 | handle pill · primary-facet tint (left border) · status ring · steps = **constellation** (~6px kind-colored dots, live step pulses 8px §16.9-FAR) · subtasks = **VOLUME CUE only** (stacked-card depth shadow + `N` badge + done/total mini-bar — see §1b). Tags SUBSTITUTE for body (§19.4 Tile). NO body text, NO per-subtask render. The far board = a readable concept map of task chips that *show their scale*. |
| **L1 THREAD** (closer) | 150–320 | 0.54–1.14 | + status label · model chip · 1-line digest · steps = **thread** (12px circles + kind glyph on a vertical spine, 1-line step TITLES readable, loops render as S-curve **coil** + iteration badge, §16.9-MID) · subtasks = **handle chips** (names + status dot, still compact — one line each, no bodies). Active step = pulsing ring; spine solid-before/dashed-after. **Still titles only — no bodies.** |
| **L2 CARDS** (near) | 320–560 | 1.14–2.0 | steps = **mini step CARDS** (~160×72: kind strip + title + 2–3 body lines markdown + iter badge, §16.9-NEAR) stacked on spine · subtasks **EXPAND to mini-cards** in the super-cell · task **body fades in CLAMPED** (~5 lines + fade) + intent · model rationale collapsed. Internal scroll engages if overflow (node-only, §24.2). |
| **L3 READING** (fallen in) | > 560 | > 2.0 | task body **unclamped** · full step cards (full milestone text/iteration) · ALL subtask bodies expanded · model rationale expanded · tab/agent link. Focus-assist fires (see §2). This is "all the way in." |

§24.4's three named levels map: **far = L0**, **closer = L1**, **near = L2+L3** (L3 is the
continuous "fill to full" tail of near, gated by dominance not just zoom).

**Crossfade, not pop:** in each ±10% boundary zone the outgoing layer fades out while the
incoming fades in (opacity), plus a 4px `translateY` surface-rise so detail "surfaces" toward
you (parallax materialize feel). **Zero geometry reflow during crossfade** — frame is fixed,
only opacity + tiny rise animate. This is what makes zoom feel like focus pulling in, not a
discrete mode-swap.

## 1b. Subtask VOLUME — scale without flattening (Andrey addendum)

Subtasks must NOT eat space at overview. From bird's-eye a task shows its **volume** (that
it's big / how done it is) **compactly**, without rendering each subtask. Use the canvas
spatially — zoom IS the level-of-detail; never flatten everything always-visible.

**Volume cue ladder (folded into §1):**
- **L0 GLYPH (far) — depth cue, zero per-subtask cost.** The task chip renders as a **stacked-
  card silhouette**: 2–3 offset drop-shadow layers behind the chip whose count/offset encodes
  magnitude (0 subtasks = flat chip; few = thin stack; many = deep stack, capped ~3 visible
  layers so it stays compact). Overlaid: an **`N` badge** (subtask count) and a **done/total
  mini-bar** (~24px, filled = done ratio). One glance = "big task, ~half done" — no subtasks
  drawn. Tasks with 0 subtasks are visually flat → the *contrast* makes heavy tasks pop.
- **L1 THREAD (closer) — roster, still one-line.** The depth stack resolves into a compact
  **subtask roster**: handle chips, one line each (name + status dot), capped ~5 with a
  `+k more` overflow chip. Volume is now legible as a short list; bodies still hidden.
- **L2 CARDS (near) — expand.** Subtasks become **mini-cards** (handle + status + 1-line
  digest) in the reserved super-cell. The stack metaphor "fans out" into real cards (a literal
  on-zoom unfold of the L0 silhouette — same spatial gesture).
- **L3 READING (fallen in) — full bodies.** Every subtask card shows its full body + its own
  step thread (subtasks have steps, §16.9/§13.6).

**Why a depth/stack cue (not just a number):** a bare `[7]` badge reads as flat metadata;
the stacked-card silhouette reads as *physical volume* pre-attentively, so the overview board
communicates "where the big work is" before you read a single number — and the fan-out on
zoom is the same object deepening, reinforcing "I fell INTO this stack," not "a tabl opened."

**Reserve-at-deepest (§0.2) still holds:** the super-cell room for fanned subtask cards is
pre-allocated, so the L0 stacked silhouette sits in space that *will* hold the fanned cards —
the unfold animates into already-reserved canvas, no neighbor reflow. Overflow beyond the
typical-deepest reserve scrolls node-only (§24.2). So volume is honest at every band:
silhouette → roster → fanned cards → full bodies, all within one fixed footprint.

## 2. The "falling in" FEEL

**Primary = pure camera-zoom content reveal** (honors §24.1: one behavior, no modes). Zooming
the camera toward a task continuously materializes its interior via the ladder above. That
alone is the core feel.

**Augmented by a lockless dominance focus-assist** (the "fall in" polish): when ONE task fills
> ~60% of the viewport area AND is at L2+:
- neighbors soft-dim to ~0.5 (the world recedes),
- the task gains a faint elevation shadow + z-lift (rises *visually* above neighbors — **no
  footprint change**, pure z-order + shadow, §22.1 safe),
- a subtle vignette draws the eye inward.
Fully continuous and reversible — zoom out and it releases. **No lock, no breadcrumb commit.**

**Two coexisting gestures, same ladder:**
- **AMBIENT drill (§24.4)** = zoom, lockless, dominance-assisted. The default "fall in."
- **COMMITTED focus (§20.5)** = double-click / `canvas focus` — pins the camera, dims harder,
  writes a breadcrumb, holds so you can read without holding the zoom. The deliberate twin.

**Composition with SPEC|PLAN|tasks (§24.3) + one-mode (§24.1):** the SAME ladder runs on every
node simultaneously (one behavior). Band is per-node-size, so as you zoom into the tasks
column the spec/plan nodes reach their own near band too — but they're off-screen/edge-dimmed,
so only the task you centered "fills." Dominance focus-assist is what singles out the one task
you fell into among its banded siblings. Coverage edges (spec-block ↔ plan-row ↔ task output,
§24.3) stay drawn to dimmed neighbors so context is never severed (§20.1 invariant).

## 3. Coming OUT (and not getting lost)

- **Zoom out reverses the ladder** — content recedes L3→L0; hysteresis kills flicker.
- **ONE flat canvas, never modal (§20.1):** zooming out ALWAYS re-reveals the same spatial map.
  Muscle memory of where the task sat is preserved — there is no drill-stack to unwind. This is
  the strongest anti-lost guarantee.
- **Breadcrumb HUD (§20.5):** `Plan:<handle> ▸ <section-band> ▸ <task-handle>`, driven by the
  viewport-center dominant node. Falling into a task appends its handle; zoom out pops it.
- **Minimap (§23.4 #5):** viewport rect shrinks into the task's band; the dominant task cell
  highlights — answers "where am I" spatially.
- **One-action escape:** when a task is dominant (L2+), a corner `⤢ fit board` chip + `Esc`
  both restore the saved board viewport (§3) in one move — guaranteed exit even if disoriented.

## 4. Overlap / readability when the interior deepens

Frame is fixed (§22.1/§24.4), yet steps + subtasks + body need room. Resolved by THREE
mechanisms — never by growing the box or reflowing layout:

1. **Reserve-at-deepest packing** (§0.2): the cell's bbox is allocated at deepest-LOD size up
   front, so deeper content fades into *already-reserved* space. Overlap is structurally
   impossible. Far-view whitespace is the only cost, and it's sub-pixel at far zoom.
2. **Internal scroll, node-only (§24.2):** content beyond the reserved cell (long bodies, >~6
   step cards) scrolls INSIDE the frame. Wheel over the node scrolls only that node, never
   hands off to canvas pan — even at the scroll boundary (§24.2 removes §23.6 release). A subtle
   scrollbar appears only at L2+ when there's overflow.
3. **Elevation + contrast, not enlargement:** the focused task rises above neighbors via
   z-order + shadow (§2) and neighbors soft-dim — so its edges read against a calmed
   background. Readability comes from CONTRAST + ELEVATION + SCROLL, never a bigger footprint.

**Subtasks specifically:** handles always live in the parent body (L1+); their BODIES (L2+)
render in the **super-cell** reserved to the right (§20.3.7) — part of the cell's pre-allocated
footprint, so no overlap and the parent card stays uncrammed. (Literal §24.4 reading "subtask
bodies inside the task footprint" is satisfied because the super-cell IS part of the task-world
cell footprint; the alternative — everything inside one scroll body — is noted but rejected as
crammed.)

## 5. Interplay with status emphasis (§24.5)

Status weighting is **orthogonal to band and multiplies at every zoom**: in_progress =
highlighted (accent ring/glow), done = dimmed ~0.7, blocked = warning tint, pending = normal.

- **Attract, never hijack.** At L0 the in_progress task's handle/dot **pulses** (extending the
  §16.9 live-step pulse to the task) — it's the eye magnet, so the user naturally zooms toward
  it. That is the auto-suggest: visual pull, not camera motion.
- **NO automatic camera movement** by default — auto-drilling/moving the camera under the user
  is disorienting (strong no). Instead offer a **one-key "jump to active"** (camera-fly-to-fit
  the live task, entering L2) + a soft "ping" ripple + minimap/breadcrumb flag when a task goes
  in_progress or a step reports. Opt-in motion only.
- **Fractal status cue:** once you've fallen into a task, the in_progress STEP inside it
  (Thread & Coil active-step pulse, §16.9) is the within-node magnet → live task among tasks,
  then live step among steps. One consistent "follow the live work" cue at both scales.
- **Optional "follow the work" mode** (default OFF): a `tail -f`-style toggle where the camera
  *does* gently track the active task. Explicitly opt-in, never the default.

## 6. Alternatives considered

- **A. LOD by raw camera scale** (single global threshold). REJECTED as primary: a small task
  and a huge spec node would cross "show body" at the same scale, but the small task is still
  unreadable on screen. Kept only as the cheap fallback if measuring apparent size is awkward.
- **B. Grow-with-content + push neighbors** (cell grows as you drill, sweep pushes others
  down). REJECTED: violates §22.1 fixed footprint and causes layout motion *while zooming* —
  the exact jitter the spec hates.
- **C. Internal-scroll-only at a MID-sized cell** (no reserve-at-deepest; everything past MID
  scrolls). Viable, less far-view whitespace, but at "near" you can't see steps + subtasks +
  body together without scrolling — more friction. **Hybrid pick:** reserve a GENEROUS-but-
  bounded cell (typical deepest: clamped body + ~6 step cards + ~3 subtask handles) AND scroll
  the long tail. Best of both.

**PICK:** apparent-size LOD driver (A's robust form) + reserve-at-typical-deepest + internal-
scroll tail (C hybrid) + lockless dominance focus-assist + attract-don't-hijack status. Pure
zoom reveal as the feel; double-click stays as the committed/locked twin.

## 7. Verification (Playwright, §16.6 / §23.7)

Assert across appW bands (L0/L1/L2/L3 — e.g. scales 0.4 / 0.8 / 1.5 / 2.5 on a 280px task):
fixed node footprint at every band (bbox identical ±2px); content-LOD changes (constellation →
thread+titles → step cards+bodies); no overflow outside the frame (overflow scrolls, node-only);
no node-node overlap (reserve-at-deepest); in_progress brighter than done at every band;
breadcrumb appends task handle when dominant; `Esc`/fit-board restores viewport.
