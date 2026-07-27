# Canvas WORLDS v3 — plan-centric composition + circle-pack task cloud + stable slots

<!-- revision: 2 -->

Design proposal answering Andrey's 7 directions. Builds on §22 (fixed footprint), §26
(W0–W3 appW worlds), §31 (no camera move on live change), §32 (worlds-v2). NO legacy/
band-aid — clean structured logic. Each point: visual + interaction, the W-band it lives
in, the math, and how it composes with the foundational rules. Alternatives + my pick where
there is a real fork.

Driver recap (unchanged, §25.1/§26.2): `appW = footprintWorldWidth × cameraScale`. Footprint
is FIXED across zoom; only PAINTED content changes inside it. Every object (spec, plan, task,
stride, section, work-point) is its own appW-driven micro-world with the SAME 4-band contract.

---

## 1. PLAN-CENTRIC COMPOSITION — plan is the sun, spec left, tasks right

**The inversion.** Today SPEC is the widest node (440) and PLAN is a thin index rail (320).
v3 flips it: **PLAN is the largest object and the gravitational center**; SPEC is a smaller
document node to its LEFT; the TASK CLOUD is a disk to its RIGHT.

```
        ┌──────────┐                                   ╭───────────────╮
        │          │      ┌───────────────────────┐    │   ·  ▢   ·    │
        │  SPEC    │◀────▶│                       │───▶│  ▢   ●(active)│
        │ (WHAT)   │ 1:1  │     PLAN  (HOW)       │ ▷  │ ·   ▢   ▢   · │
        │ sections │ rows │   central · LARGEST   │ ▷  │   ·  ▢  ·     │
        │          │◀────▶│                       │───▶│  TASK CLOUD   │
        └──────────┘      └───────────────────────┘    ╰───────────────╯
         specW×H            planW×H  (biggest)            disk, R∝√n
```

**Size hierarchy (hard invariant).** `plan > spec ≈ tasks`, and a task can NEVER reach
spec/plan size — "they're just tasks":

```
planW = 680, planH = 860            // the focal node, biggest in BOTH axes
specW = 440, specH = 720            // smaller; the design doc
taskMaxW = 240, taskMaxH = 300      // ceiling; bounded growth (§32.4) clamps here
INVARIANT:  ½·√(taskMaxW² + taskMaxH²) < min(specW, specH, planW, planH)/2
            → a task's enclosing circle is always smaller than the smallest big node.
```

Enforce in `taskFootprint()` by clamping to `taskMaxW/taskMaxH` (already there) and asserting
the invariant in `settings.ts` (a unit test, not a runtime check).

**Δ from current `settings.ts` (these are CHANGES, not new fields):**
```
planWidth   320 → 680      // plan goes from thin index rail to the focal mass
specNodeW   440 (keep) but height now from sectionNodeHeight; spec moves to negative-x
taskMaxW    276 → 240
taskMaxH    380 → 300      // FORCED by the invariant — at the current 380 it fails:
                           //   ½·√(276²+380²) ≈ 235 > 440/2 = 220  ✗
                           //   ½·√(240²+300²) ≈ 192 < 220          ✓
```
The `taskMaxH` reduction is not cosmetic — keeping 380 breaks the "a task is always
smaller than the smallest big node" hard invariant. Shrink it, then lock it with the unit test.

**Spatial math (composition origin).** Plan placed first; spec and cloud are vertically
centered on the plan's mid-line so it reads as a balanced triptych with the plan as the
massive center panel:

```
planBox   = { x: 0, y: 0, w: planW, h: planH }
axisY     = planH / 2                                  // the composition's horizontal axis
specBox   = { x: -(columnGap + specW), y: axisY - specH/2, w: specW, h: specH }
diskCenter= { x: planW + columnGap + R, y: axisY }     // R = cloud radius (§2)
```

**The 1:1 spec↔plan edges no longer force equal heights.** `railAnchorOffsets(n, h, s)`
already distributes N section rows inside *each* node's own height. So spec rows sit at
spec's offsets, plan rows at plan's offsets, and the same-section edge is allowed to be
diagonal across the differing Ys. This frees the size hierarchy (spec smaller than plan)
while keeping the section mapping. **No code in the offset math needs to change — only the
two node widths/heights and the x-placement (spec moves to negative-x, left of plan).**

**LOD band.** Composition is structural, lives at every band. At **W0** the three masses
read as territories (spec block · plan block · task disk) with the plan dominant. Zoom only
moves the camera between them (§22.1).

**Composes with §31:** plan is placed at a fixed origin; live mutations never re-place the
plan/spec — only the cloud re-slots internally (§2), camera untouched.

---

## 2. TASK CLOUD = CIRCLE PACK (the math)

Replace the column-major cloud with a **Fermat-spiral (golden-angle) phyllotaxis** that packs
variable-size square tiles into a disk, non-overlapping, deterministic, round.

### 2.1 Why phyllotaxis (and why not the others)

| Candidate | Verdict |
|---|---|
| **Phyllotaxis / sunflower (Vogel)** | **PICK.** Deterministic, append-stable (new tile = next index, never moves earlier tiles), naturally round, uniform density. |
| Force-directed relaxation | REJECT — non-deterministic, jitters on every tick, violates §31. |
| Concentric rings | REJECT — ring capacity shifts with variable sizes → reshuffles. |
| Fermat + collision | FOLD IN — used as the non-overlap *guarantee* on top of phyllotaxis placement. |

### 2.2 The algorithm

Golden angle (the irrational that gives maximal spacing):

```
ψ = π(3 − √5) ≈ 2.399963 rad ≈ 137.50776°
```

Each task tile j (in stable order) has footprint (wⱼ, hⱼ). Its enclosing radius + gap:

```
ρⱼ = ½·√(wⱼ² + hⱼ²) + g            // g = inter-tile gap (~10px)
aⱼ = π·ρⱼ²                          // disk area the tile claims
```

**Area-accumulated radius** (generalizes Vogel's r=c√i to variable sizes → uniform areal
density → round cloud):

```
Cⱼ = Σ_{k<j} aₖ                     // cumulative claimed area before j
rⱼ = √( (Cⱼ + aⱼ/2) / π )           // radius where disk area == area used, tile-centered
θⱼ = j·ψ + θ₀
centerⱼ = diskCenter + (rⱼ·cosθⱼ, rⱼ·sinθⱼ)
```

**Non-overlap guarantee — ordered outward-only collision resolve.** Phyllotaxis spacing is
correct in the *mean*, but a tiny tile beside a huge one can still touch. After computing rⱼ,
sweep outward (j ascending): while tileⱼ's enclosing circle intersects any already-placed
tile k<j, bump `rⱼ += penetrationDepth` and recompute centerⱼ. Because the sweep only ever
pushes the CURRENT tile outward and never touches k<j, it stays deterministic and
append-stable.

```
R_disk = max_j (rⱼ + ρⱼ)            // grows ~√n: 100 tiles ≈ 10× a single tile radius — compact
```

### 2.3 Stability vs status-centrality — the real fork

Andrey wants BOTH a round cloud and live work at the eye, AND no jitter on add/remove (§31).
Index choice decides this:

- **Alt A — creation-rank index (append-only):** index never reused; new task → outermost;
  removed → leave hole. **Zero ripple ever** (existing tiles never move). Status shown by
  emphasis only (§24.5 ring/pulse), not position. Maximally §31-pure, but active work isn't
  spatially clustered.
- **Alt B — status-composite index:** index = rank by `(shelfRank, priority, recencyBucket,
  createdAt, id)`. Active = low index = inner; done coils to the rim. Matches §32.10's
  "active at the eye", but a status change re-ranks one task → downstream radii ripple
  outward (animated 300ms). Visually a settle, camera still fixed — but it's motion on every
  status flip.
- **Alt C (RECOMMEND) — two-zone sunflower:** a small **active core** (innermost K slots,
  K≈6–13 ≈ first 1–2 rings) reserved for `in_progress` tasks, status-ordered; everything else
  is the **history spiral** in creation-rank (append-stable). A task going active animates
  INTO the core (one tile moves); going done LEAVES it (one tile moves). Ripple is bounded to
  ≤K, never the whole disk. **Round shape + active-at-center + stability — all three.**

```
slot(task):
  if task.status == in_progress:  core spiral, index = rank within active set (≤K)
  else:                            history spiral, index = K + creationRank(task)
```

Recommendation: **Alt C.** It is the only one that satisfies all of {round, active-centered,
§31-stable} simultaneously, with motion bounded to the handful of active tasks.

### 2.4 Edges from plan section outputs into the cloud

Section→task edges (plan right-edge output dot → task left/inner edge) will cross inside a
single round disk. Two sub-options: (a) keep the global sunflower and **bundle** edges toward
the plan output dots (thin, low-opacity, grouped) — round wins, edges recede to chrome; (b)
**sectored disk** — angular sector per section, local spiral inside each, short edges, but
sparse sectors look ragged and it breaks the single round shape. **Pick (a):** Andrey asked
for a round cloud; edges are secondary. Use primary-section only as an angular tiebreak so a
section's tasks loosely arc together without hard sectoring.

### 2.5 LOD band & composition

The disk is W0 structure: at **W0** each tile is a mass/silhouette (status color + ring count
§7); the disk reads as a round constellation. Tile interiors reveal per the tile's own appW
(§3). Footprint fixed (§22.1 / reserve-at-deepest §25.2) → tiles never resize on zoom → the
pack is computed once per data mutation, never per frame. Live add/remove (Alt C) = bounded
animated settle, **camera byte-identical** (§31).

---

## 3. TASK ZOOM-IN — three FIXED slots, content swaps per band

A task tile is a **fixed 3-row grid** reserved at the deepest LOD (§25.2). The slots NEVER
reflow on zoom; only their content detail changes per band. The grid template is constant per
card:

```
grid-template-rows:
  [TITLE]  auto         // fixed: name + status + tags
  [DESC]   1fr          // flexes to fill reserved height
  [WORK]   auto         // fixed: the work-point row (the circles — confirmed RIGHT)
```

Replace TaskCard's current single flex column (where body + snake reflow into each other)
with this rigid grid. Per-slot content function by band:

| Slot | W0 (far) | W1 | W2 | W3 (closest) |
|---|---|---|---|---|
| **TITLE** | handle + status chip + tags | + type + model chips | (same) | (same) |
| **DESC** | 1-line digest (intent first line) | 3–4 line clamp | clamped markdown | **full rendered markdown, scroll appears ONLY here** |
| **WORK** | dots constellation | beads + 1-line labels | beads + callouts | bloom → work-point micro-worlds (§4) |

**Rules:**
- Scroll is forbidden until **W3** and lives ONLY in the DESC slot (§24.2 node-only wheel).
- Content crossfades (opacity + 4px rise, §25.4); slots' y-bands are byte-fixed.
- The WORK slot height is reserved at its deepest (bloom) extent so revealing strides never
  pushes DESC (no reflow). DESC `1fr` absorbs the slack at shallow bands.

This is the structural fix for "layout reflows on zoom": a fixed CSS grid + per-slot band
renderers, not a flex stack.

---

## 4. WORK-POINTS (strides) — inscribed text + "fills screen" stop

### 4.1 The bug

Today `StrideSnake` renders labels as absolutely-positioned divs OFFSET above/below each bead
— text AROUND the point, fighting the geometry, unreadable at scale. Fix: **inscribe text
INSIDE the circle**, sized to the bead's *apparent* radius.

### 4.2 Inscribe geometry

A circle of apparent radius ρ (px = beadWorldR × cameraScale) contains a max inscribed square
of side `s = ρ·√2 ≈ 1.414ρ`. The text container is that centered square, clipped to the
circle. Font size steps **discretely** per micro-band (never continuous counter-scale, §18) so
text is always a designed readable size:

```
textBox = centered square, side = √2 · ρ_apparent
fontStep = pick from {11, 13, 15} by micro-band (s1/s2/s3)
```

As the camera zooms a single bead, ρ_apparent grows → the inscribed box grows → more text fits
— without the bead's WORLD footprint changing (§22.1).

### 4.3 Each work-point is a leaf MICRO-WORLD (4 bands, finite depth)

Same appW contract, indexed `s0–s3` (§32.5), progressive reveal tags → text → full structure:

| Band | appW of bead | Inscribed content |
|---|---|---|
| **s0** | dot | colored dot, no text (mass only) |
| **s1** | small | kind word, 1 line, centered in circle |
| **s2** | medium | title + 2-line excerpt, inscribed; near bead sprouts a callout |
| **s3** | **fills screen** | bead **blooms** into a full reading panel anchored at the bead's world position: heading / body / links / author·Tab·session·timestamp, internal scroll if needed |

**The deepest-band stop (limit the infinite-deepen).** When the work-point's appW ≥ ~0.8 ×
viewport width, clamp the micro-LOD at s3 — the panel IS the terminal. No reveal deeper than a
comfortable full-screen read. This bounds the micro-world to 4 bands (mirrors the
"bounded-to-two-levels" invariant §20.1, extended to leaves).

**No flying.** The bead keeps its slot on the snake at every zoom; the camera moves, the bead
stays put in world space. Bloom space is RESERVED in the task footprint (§32.5) so s3 never
reflows neighbors. The s3 panel is anchored at the bead's world coords — it does not detach
or float.

**LOD band.** Work-points live in the task's WORK slot, active across W1–W3 of the parent and
s0–s3 of themselves (the parent band gates which micro-band is even reachable).

---

## 5. SECTIONS — full content, never truncated

Sections (rows/blocks inside SPEC and PLAN nodes) are cut today. Make each section a
**sub-world** with its own appW band (`sectionAppW = sectionBlockWorldWidth × cameraScale`),
progressive reveal like tasks, text shrinking in designed steps as the plan grows / camera
approaches:

| Band | Section content |
|---|---|
| **W0** | section title + tags + coverage glyph (the rail row) |
| **W1** | + first-line abstract / digest |
| **W2** | + section body, clamped markdown (paged blocks, keep-with-next) |
| **W3** | **FULL paragraphs / rendered markdown (mermaid/code), node-internal scroll — NEVER truncated, never ellipsis** |

The "sections are cut" bug = currently clamped at all bands. Fix = **W3 renders the unclamped
body with node-internal scroll** (§26.3 markdown treatment: visible block structure, not a
flattened blob). Font steps discretely per band (§18 discrete designed sizes), so text
"shrinks in steps" as Andrey describes, never a continuous stretch.

SPEC owns the full prose (largest text); PLAN section rows show the HOW digest (§6). Both ride
their parent node's appW but each section block also crosses its own sub-band so a tall node's
top sections can be at W2 while the camera sits on them and the rest stay W1.

---

## 6. SPEC vs PLAN — distinct content (fix the repeated titles)

**Diagnosis.** The plan repeats spec section titles because the plan→canvas ingestion copies
*structure* (titles, ids) but NOT the orchestrator's rich working-markdown plan body. So plan
section bodies are empty/title-only → the canvas (which CAN render bodies at W2/W3, §26) shows
nothing new → looks like a duplicate of the spec.

**The distinction (design contract):**
- **SPEC = WHAT.** Section title + prose: the requirement, the goal, the constraint, the
  design rationale. (This doc.)
- **PLAN = HOW.** Per section: the **approach**, the **ordered task breakdown** (handles), the
  **decisions/strides at plan level**, **coverage rationale**, **open questions**. None of
  this is in the spec.

The 1:1 spec↔plan edge (§24.3) then shows mapping WHAT→HOW, not WHAT→WHAT.

**What is missing (flag to orchestrator):**
1. **Ingestion gap:** the orchestrator writes a rich markdown plan (the working doc) that
   never lands in plan section `.md` bodies. The §17/§33 externalized-body machinery exists —
   the fix is mapping the orchestrator's plan-markdown sections into plan section bodies,
   keyed by section id, kept DISTINCT from spec bodies.
2. **Plan-level strides:** decisions belong on the plan/section, not only on tasks. Plan
   sections need their own Thread & Coil stride lane (§25.5 referenced this).
3. **Coverage rationale per section** — why this section is/ isn't covered, currently absent.

**Visual signal for the gap:** if a plan section body ≈ its spec title (under-filled), paint a
**"thin section" warning glyph** on the plan row so the missing HOW is visible on the board,
not silently duplicated.

No canvas-render change is needed once bodies are filled — §26 already renders them. This is an
INGESTION + DATA-MODEL fix, surfaced visually.

---

## 7. EMERGENCE / LOOPS — passes & re-entry on every object

Every object (spec, plan, task, stride, section) is emergent and re-enterable. Express
"this is pass N / loop" uniformly.

### 7.1 Data model

```
iteration:  number          // current pass, 1-based (milestones already have this)
loop_id?:   string          // groups passes of the same re-entered work (§16.9 coil)
lineage?:   string[]        // ids/refs the current pass derived from (the chain)
```

Add to spec/plan/task/section, generalizing what strides already carry. Re-open semantics
(append-only events, §33): re-opening a `done` task does NOT clone it — same id, a
status-change event + a NEW stride **generation** appended; `iteration++`. Plan execution
loops = the plan itself carries a pass counter; a new plan pass may re-open tasks. History is
never lost (append-only log folds to current state).

### 7.2 Visual signal — the GROWTH RING / COIL motif

Loops are spatial, fractal, and pre-attentive:

| Band | Loop signal |
|---|---|
| **W0** | **growth rings** on the tile/bead — concentric rings = pass count (tree-ring metaphor); a many-pass task reads as "heavily revisited" at a glance |
| **W1** | `↻N` iteration badge + last-pass timestamp |
| **W2** | collapsed pass chips (pass 1 · pass 2 · …) |
| **W3** | full pass timeline — each pass = a stride **generation**; the snake shows N **coils** (loops literally coil back, §32.5); expandable |

### 7.3 Composes with everything

- **Spiral (§2):** re-opening a task flips it to `in_progress` → animates INTO the active core
  (Alt C) → "re-entry pulls work back to the live center"; done again → coils back to the rim.
  **The sunflower spiral IS the loop made spatial.**
- **§31:** re-open is a user/agent action → in-place pulse if on-screen, off-screen edge
  signal if not — camera never moves.
- **Fixed footprint (§22):** rings/coils are paint layers; they never change the object's
  footprint. The deepest-band pass timeline scrolls node-internally (§24.2), no growth.

---

## Summary of decisions (recorded as strides)

1. **Composition:** plan central & largest; spec left, cloud right; size hierarchy enforced by
   a clamp invariant; 1:1 edges go diagonal so heights can differ.
2. **Circle-pack:** golden-angle phyllotaxis + area-accumulated radius + outward-only
   collision resolve; **two-zone sunflower** (active core + creation-rank history spiral) as
   the recommended index for round+active+stable.
3. **Task slots:** fixed 3-row grid (title/desc/work) reserved at deepest; content swaps per
   band; scroll only at W3 in DESC.
4. **Work-points:** inscribed text (√2·ρ square, discrete font steps); 4-band micro-world;
   s3 "fills screen" terminal stop; no flying.
5. **Sections:** sub-world with own appW; W3 = full untruncated markdown + node scroll; font
   steps discretely.
6. **Spec vs Plan:** spec=WHAT, plan=HOW; the bug is an ingestion gap (orchestrator markdown →
   plan section bodies) + missing plan-level strides/coverage; flag thin sections.
7. **Loops:** `iteration/loop_id/lineage` on every object; growth-ring (W0) → ↻N (W1) → pass
   chips (W2) → coiled stride generations (W3); re-entry pulls a task into the active core.
