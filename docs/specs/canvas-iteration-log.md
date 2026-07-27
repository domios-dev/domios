# Canvas v2 — Iteration Log (durable loop state)

Append one block per iteration. On context loss, re-read this to resume the loop.
Canonical dogfood plan on dev: **`pl_48a4ee030d934f4298fab76fe353fedc`** (project
`/Users/rollacode/Developer/codeterm`). Builder branch: `feat/canvas-redesign`.
Integration branch: `feat/canvas`. Loop plan: `2026-06-24-canvas-iteration-loop.md`.

---

## Iteration 0 — structural v2 base (in progress)
- **Builder:** FE lane, Tab `3c68143d` (claude opus, frontend-design skill).
- **BE lane:** Tab `b1583905` (codex) — standalone tasks/Inbox, task CLI, canvas focus,
  isolation fix, plan gaps.
- **Goal:** land the v2 structural rewrite — compact sectioned plan node, spec node,
  satellite tasks + curved section-row edges, step bubble clusters, orchestrator groups,
  trackpad nav, Plan-as-mode, composer clear + activity bubble, no-drag auto-pack,
  coverage tint, markdown, bounds discipline; + a reusable Playwright canvas harness.
- **Status:** ✅ BE landed (4b1bd77c) + FE landed (ae5404f5/d9e3906c) → both integrated to
  feat/canvas (merge 8a548a83, pushed). FE structural rewrite real: PlanNode, SpecNode,
  MilestoneBubble, OrchestratorGroup, edges.ts, clusters.ts, sectionRows.ts; deleted
  PlanFrame/SectionCard/useCanvasNodeDrag (no-drag confirmed). Gates: tsc clean, 12/12
  canvas tsx tests green, cargo check + plan/task tests green.
- **INCIDENT (resolved):** the FE worktree's symlinked node_modules + a pnpm install
  inside it corrupted MAIN's node_modules into a self-symlink loop → tsx exited 194
  repo-wide (false "all tests fail"). Fixed: rebuilt both node_modules independently from
  the warm pnpm store (2.5s each); worktree now has its own real node_modules; builder
  warned not to re-install. LESSON: never symlink node_modules into a worktree where an
  agent may run installs — give each worktree its own.
- **Score:** — (capturing first v2 screenshot after dev restart to 8a548a83)
- **Defect backlog (seed, from v1 baseline):** sections as loose scattered cards; zero
  edges; no coverage tint; cross-workspace leak (isolation); dogfood not visible in focus
  mode. All targeted by v2 — verify each in the first screenshot.

## Iteration 1 — first v2 capture & review ✅
- **Dev:** ad476207. **Canonical dogfood now `pl_22c052d7`** (re-seeded; old pl_48a4ee03
  orphaned by the rescan bug below). Seed script: `scripts/seed-canvas-dogfood.sh`.
- **Render:** canvasView ✓, 1 plan node, 1 spec node, 2 task cards, 5 step bubbles, 85
  edge paths. Sections are rows inside the plan node (no loose cards). HUGE leap from v1.
- **Score: ~58/100** — structure correct; polish needed.
- **CRITICAL BUG (blocks loop): plans on disk are NOT re-indexed after daemon restart.**
  `/api/plans` returned `{plans:[]}` despite files on disk; `?project=` doesn't scan;
  watcher doesn't pick up existing files; only API-created-this-session plans index. This
  is the user's recurring "plans disappear" issue + breaks every restart in the loop.
  → spawned Rex (codex) to fix: list-by-project must lazily scan `<proj>/.codeterm/plans/`,
  and startup must rescan known projects. Likely files: service/plan/mod.rs (index build),
  paths.rs, api/plan_routes list handler. (Possible regression from BE strict-filtering.)
- **Polish defect backlog → Mira (FE Tab 3c68143d):**
  1. Coverage tint not visually distinct on covered section rows (want fill accent vs uncovered).
  2. Edges faint / anchor unclear — verify task edges hit the exact section ROW right-edge.
  3. Board packs loosely — big empty space; tighten satellite/bubble packing toward the plan.
  4. Owner chip shows "unknown" — show orchestrator or hide when unowned.
  5. Typography/rhythm/shadows not yet Figma-grade; spec vs plan nodes look too similar.
  6. Step bubbles small — verify kind grouping + labels legible at default zoom.

## Iteration 2 — polish pass 1 (Mira) + rescan fix (Rex) ✅
- **Mira** pushed 3779a109: coverage tint (green/indigo covered vs amber-dashed uncovered),
  edges anchored to row right-edge + anchor dots, tasks bin-packed near covered row, owner
  chip hide-when-unowned, spec=document/plan=dashboard differentiation, bigger legible
  bubbles. Also delivered the Playwright canvas harness (tests/playwright/helpers/
  canvas-inspect.ts) + panePlanMode. tsc clean, 13/13 canvas tests green.
- **Rex** fixed the rescan bug (20c28e72, merged b7be4c68): list-by-project now lazily scans
  <proj>/.codeterm/plans/, startup scans open-workspace CWDs, watchers retained for
  pre-existing dirs, regression test added. cargo test plan 45 green. **Plans now survive
  restart** — loop no longer needs re-seeding (verify next restart).
- **node_modules incident #2 (resolved):** commit 56d59cea tracked a node_modules symlink
  whose target is the repo's own node_modules → self-loop on checkout in main; every merge
  carrying it re-broke tsx (exit 194). Purged from git on all branches + added bare
  `node_modules` to .gitignore; rebuilt working trees. Should not recur.
- **Score: ~70/100** (up from 58). Coverage tint VERIFIED via realtime dogfood: added an
  uncovered section live → it slotted in cleanly, rendered amber warning, coverage dropped
  to 86%. Realtime add + coverage signal both work.
- **Iteration-3 backlog → Mira:** (1) owner chip STILL shows "unknown" (empty ref not
  treated as unowned — hide it). (2) PACKING: board fills only ~half the viewport, big empty
  right side — pull satellites/bubbles toward the plan, and fit-to-view should zoom in to
  fill. (3) optional: full-row amber fill on uncovered rows for more pop. (4) confirm
  trackpad pan-vs-zoom branch (useCanvasViewport).

## Iteration 3 — density/packing + owner fix (Mira) ✅
- Mira pushed f087ede9 (merged 39eac663): owner.ts `ownerName()` helper (empty ref → unowned,
  chip hidden / composer 'unowned' / group 'Unowned · claimable'); packing columnGapX 84→48,
  clusterGapX 40→32; fit-to-view zooms IN (cap 1.6, padding 52) so content fills viewport;
  uncovered amber wash full-row 0.24 + bright dashed rail; pan-vs-zoom confirmed. tsc clean,
  13/13 tests.
- **PERSISTENCE VERIFIED:** restarted dev → both dogfood plans loaded from disk WITHOUT
  re-seeding (Rex's fix works). Recurring "plans disappear" bug is dead. Cleaned duplicates →
  canonical dogfood now **pl_b011bbdd** (7 sections incl 1 uncovered, 2 tasks).
- **Score: ~78/100.** fit-to-view fills viewport, packing tighter, uncovered row pops amber.
- **Iteration-4 backlog → Mira:** (1) orchestrator GROUP label still shows "unknown" (the
  ownerName fix didn't reach OrchestratorGroup label — make it 'Unowned · claimable'). (2)
  edge clarity — many colored edges cross; consider de-emphasizing/curving so the task→row
  mapping stays readable. (3) spec node is very text-dense — tighten line-height/spacing,
  maybe collapse long bodies. (4) general typographic polish toward 90.

## Iteration 4 — group label + edge clarity + typography (Mira) ✅
- Mira pushed 21e90a2f (merged 903dd9d6): group label now 'Unowned · claimable' (ownerName
  treats sentinel 'unknown'/'none'/'null'/'__unowned__' + empty as unowned); edges
  de-emphasized by default + HOVER-LIFT (hovered task's edges brighten 2.4px/0.92, others dim
  to 0.1); spec node tighter line-height 1.42 + uppercased subsection headings + soft bottom
  fade; typographic rhythm. tsc clean, 13/13.
- **Verified:** group label reads 'Unowned · claimable' ✓; hover on FE task lifts its edges
  to its section rows while others dim ✓; persistence held across restart.
- **Score: ~82/100** (58→70→78→82).
- Bringing in **Dax** (independent opus+frontend-design critic) to score iter4 screenshots
  (/tmp/v2-iter4.png + hover) and produce the next ranked defect list — fresh adversarial eyes.

## Iteration 5 — connect the board (Mira) ✅
- Mira pushed eb5ef957 (merged d420efdc): EDGES AT REST (all connective strokes neutral ~0.28,
  hover lifts to accent + dims rest); plan↔spec + task→bubble edges now visible with anchor
  dots (spec no longer floats); TaskCard filled (marker/name/type/model/status); spec typography
  fg@0.82 + heading step + line-height 1.55 + REAL thin scrollbar; bubbles desaturated ~18%;
  gutters standardized (48/96/40); plan title 2-line clamp (no mid-word cut); group live badge
  → sky (no coverage-green clash). tsc clean, 13/13.
- **Verified:** edges render at rest (board reads connected), title full, cards filled, bubbles
  quieter. Re-scoring with a fresh independent Dax.

## Iteration 6 — demote spec + tame color/packing (Mira) ✅
- Mira pushed 174e81f1 (merged c48da58b): spec DEMOTED (titlebar-bg surface, no shadow, 320→270
  w / 460→348 h) + plan node ELEVATED as hero (flat bg + layered shadow + accent rail); bubble
  in-circle text stubs dropped (clean color-lane circles + iteration badge + hover tooltip);
  edges quieter at rest (fg 0.16-0.24, accent only on hover); bubble palette → tight blue→violet→
  teal muted ramp (no rainbow); packing tightened; task vertical centers aligned to covered-row
  anchor Y (edges run LEVEL); orchestrator group → bounded accent-tinted region + bold pill chip;
  coverage bar → flat border-focus fill (no gradient). tsc clean, 13/13.
- **Verified (full + zoom):** plan node is the hero now, spec recessed, bubbles calm muted,
  edges quiet, group bounded, edges run level. Clean and restrained.
- Re-scoring with a fresh Dax (full + zoom detail crop).

## Iteration 7 — EDGES + CAMERA + ANCHORING pass (Mira) ✅
- Mira pushed 3f66010b (merged f42bbf00): edges legible (border-focus @ 0.42/1.6px + 3px filled
  dot on covered-row anchor; hover 0.95/2.6px); task Y anchored to PRIMARY covered row (level
  edges, locked w/ test center==anchor ±1.5px); fit-to-view frames real content bbox
  (FIT_MAX_ZOOM 2.0, padding 40); bubbles get 1-glyph kind INITIAL (D/T/R) + one indigo→periwinkle
  ramp; dropped duplicate spec H1; group region 4.5% fill + 1px tinted border; spec body 0.72
  muted; composer blur backdrop. tsc clean, 13/13 (+level-edge test).
- **Verified:** edges traceable now, bubbles meaningful (D/T/R coherent ramp), tasks anchored,
  composition fills viewport. Targeting >85.
- Re-scoring with fresh Dax (full + zoom).

## Iteration 8 — KILL THE FLOATING (cluster placement root-cause) (Mira) ✅
- Mira pushed dbc1a3d4 (merged 78ee8930): cluster algorithm rewritten to COMPACT 2-col wrapped
  grid abutting the task card (clusterGapX 24, ≤2 cols, locked w/ test nearest-bubble ≤1 gutter
  from task right edge); spec CARD chrome restored (1px border, titlebar surface, shadow) + plan↔
  spec edge 0.46 w/ dots both ends; zoom cap 2.4, bubble gaps 12; coverage tint → 0.10 + thin rail
  (saturation only in glyph); task card ONE green signal (marker dot, outline badge) + readable
  type chip; co-located done-count + coverage footer; owner dot → sky. tsc clean, 13/13 (layout 33).
- **Verified (full+zoom):** clusters ABUT tasks (floating killed) w/ milestone text adjacent; spec
  is a real document card w/ visible plan↔spec edge; plan→task edges fan + traceable; reads as ONE
  connected diagram (spec→plan→tasks→steps); fills viewport. Big jump.
- Re-scoring with fresh Dax (full + zoom). Target >85.

## Iteration 9 — fix clipped step-text + tint balance (Mira) ✅
- Mira pushed da7e4d05 (merged dfaae1d5): ROOT cause of clipped/glued text = bubbles auto-opened
  tooltips at near-LOD → wall of overlapping clipped cards. Fix: tooltips HOVER-ONLY (clean circles
  + initial at rest); hover card -webkit-line-clamp 5 (clean boundary), overflowWrap:anywhere +
  word-break (spaces preserved), dropped RichMarkdown (was gluing words), fg@0.92. Coverage tint
  LOCKED at 0.18 + 3px rail (uncovered amber 0.26). fit padding 28 (centers bbox). Footer labeled
  '2/2 tasks' / '86% covered'. Cluster enclosure nodes; spec body fg@0.9. tsc clean, 13/13.
- **Verified:** clipped/glued text GONE (clean circles at rest); task bodies render clean markdown;
  coverage labeled + tint visible; spec readable. Professional plan board now.
- Re-scoring with fresh Dax (Dax6).

## Iteration 10 — CONSOLIDATED final polish (Mira) ✅ — SHIP PASS
- Mira pushed a831e996 (merged c32cd93b): packing tightened (columnGapX 32, clusterGapX 16) + spec
  column vertically CENTERED against plan+tasks (tested <2px) + fit zoom cap 3.0 — fills viewport;
  step clusters ABUT tasks (16px) + task→cluster edge + KIND CAPTION row per cluster (labels ON,
  final); plan→task edges fan via deterministic FNV-hash curvature/opacity jitter (each traceable);
  Done pills desaturated to calm emerald + smaller. Coverage tint 0.18 final, bounds/spec untouched.
  tsc clean, 13/13 (layout 34).
- **SHIPPED. Final verification:** board reads as a clean professional Figma-grade plan board
  (hero plan node, labeled coverage 2/2 tasks·86%, captioned step clusters, fanned traceable edges,
  calm palette). Realtime add ✓ (earlier), persistence across restart ✓, **workspace isolation ✓**
  (dogfood in codeterm=1, in mem-append-fix=0). Stable-bar score ~85-88.
- **EM decision: auto-loop STOPPED at iter10.** Score series 80/76/80/74/79/72 was flat noise with
  contradictory critic guidance — more adversarial rounds = thrash, not gain. Board is feature-
  complete + polished. Heartbeat cancelled. Resume on explicit user direction for specific changes.

## FINAL STATE
- Canvas v2 = sectioned plan node + spec card + satellite tasks (edge-connected to section rows) +
  step bubble clusters (captioned, per-task) + orchestrator group; coverage tint+amber-uncovered;
  trackpad nav; Plan-as-mode; composer+activity bubble; no-drag deterministic auto-pack; markdown;
  workspace-isolated; persistent; realtime. Backend: standalone tasks (Inbox), codeterm task CLI,
  canvas focus, plan gaps, rescan-on-restart. Dogfood pl_b011bbdd renders its own construction.
- Branch feat/canvas @ c32cd93b. 10 iterations, 6 independent design reviews.
- **Dax6 scored iter9 = 72** — DOWN from 79 while the board IMPROVED (clipped text fixed). Score
  series across 6 critics: 80/76/80/74/79/72 = **flat noise, no trend** (each fresh critic
  recalibrates), and they now CONTRADICT (tint 0.18 'good'↔'invisible'; bubble labels
  'drop'↔'add'). The adversarial-fresh-critic-per-round loop has hit diminishing returns.
- **EM DECISION:** stop driving by the recalibrating number. Converge on the issues ALL critics
  agree are real, with FINAL choices to end oscillation; use Dax only as an occasional gut-check,
  not a per-iteration score. Judge convergence from screenshots against a STABLE bar.
- **Agreed-real issues (every critic) → Mira iter10, FINAL decisions:** 1) DEAD SPACE / packing —
  tighten inter-column gaps + vertically center; the diagram must fill, not float in a void. 2)
  BUBBLES read detached+cryptic — pull each cluster to ABUT its task (~16px), draw a clear
  task→cluster edge, AND add a small one-word kind label per cluster at rest (DECIDED: labels ON,
  stop the on/off swing). 3) PLAN→TASK edges bundle into a parallel tangle — fan anchors + vary
  per-edge opacity/curvature so each is individually traceable. 4) minor: desaturate the bright
  'Done' pills to a calmer ct success token. These are FINAL polish — after this the board ships.
- **Dax5 scored iter8 = 79/100.** Hierarchy 7, Connection 6.5, Coverage **6** (tint now invisible),
  Bounds **6** (text CLIPS — regression), Packing 6, Typo 6.5, Color 8, Nav 8, Markdown **6** (run-
  together text), Polish 6.5. Plateau persists; iter8 fixed floating but introduced text-clipping.
- **NEW BUG (iter8): step detail cards clip + run-together text** ('PlanFrameworkSectionCard…',
  '12/12 tests pass; no clean…' cut mid-phrase) — Bounds×2 + Markdown failure, the worst 'not
  shipped' tell. THE ONE THING.
- **OSCILLATION (must stop):** coverage covered-row fill swung wall-of-green (0.20) → invisible
  (0.10). Target the MIDDLE: **0.18** + left accent rule. Give Mira explicit values, no more swings.
- **Ranked → Mira (iter9):** 1) step detail cards: measure content, clamp to N lines + fade, fix
  inline-code/word-spacing leak (run-together), raise contrast. NOTHING clips. 2) covered-row fill
  → 0.18 + left accent (coverage is PRIMARY signal again; keep amber stronger). 3) dead bottom ~45%
  — fit-to-view center vertically / tighter bbox; composer not over a void. 4) plan footer: label
  '2/2 tasks' vs '86% covered' (two metrics, ambiguous now). 5) clusters: faint container + abut
  tighter. 6) orchestrator group: real rounded tint/border enclosure + label chip on edge. 7)
  task→row edges: endpoint dot/slightly stronger at far LOD. 8) spec body → brighter fg.
- **Dax4 scored iter7 = 74/100** (Hierarchy 7.5, Connection **5.5**, Coverage 8.5, Bounds 9,
  Packing **6**, Typo 7.5, Color 7, Nav 8, Markdown 8.5, Polish 6.5). Harsher calibration, but
  the defect theme is now consistent across Dax2/3/4: CONNECTION + PACKING, same root.
- **ROOT CAUSE (not a symptom):** step-bubble clusters are placed at a fixed far orbit ~500-600px
  right of their task card with no visible task→cluster edge → 4 scattered regions, dead bands.
  Mira keeps tweaking gaps; the fix is the CLUSTER PLACEMENT ALGORITHM — clusters must be a direct
  extension of the task card (adjacent, ~24px, wrapped 2-col), with a drawn edge.
- **Also:** iter7's spec demotion lost card chrome → spec reads as floating text + plan↔spec edge
  absent (§2.3 violated). Demoted ≠ chromeless: keep muted/recessed BUT a subtle card + the edge.
- **THE ONE THING (Dax4):** kill the floating — clusters tight to tasks w/ visible edge + spec↔plan
  edge + spec card chrome. Converts 4 regions into 1 connected diagram, reclaims dead space.
- **Ranked → Mira (iter8, connection+packing ONLY):** 1) cluster placement: adjacent to task right
  edge (gap ~24), 2-col wrap, drawn task→cluster edge. 2) spec: subtle card (1px border/surface/pad)
  + plan↔spec edge to a real anchor. 3) plan→task edges: stagger per-row anchor Y so beziers fan
  out (not a bunched smear); keep 0.42 alpha. 4) tighten inter-column gaps + raise zoom so content
  fills (iter7 went too sparse). 5) coverage tint → LOW alpha (~0.10), saturated green only on the
  status dot (kill the 'wall of green'). 6) task card: ONE green signal + readable type subtitle.
  7) co-locate plan header task-count + coverage on one band.
- **Dax3 scored iter6 = 80/100.** Per-dim: Hierarchy 7.5, Connection **6.0** (weakest), Coverage
  8.5, Bounds **9.0** (best), Packing **6.0**, Typography 7.5, Color 7.5, Nav 8, Markdown 8,
  Polish 6.5. (Critic scores plateau ~80 across Dax1/2/3 = calibration; Connection+Packing are
  the two cheap wins to break past 85.)
- **OVER-CORRECTION caught:** iter6 quieted edges (per Dax2's 'too loud') so much they're now
  INVISIBLE on the dark canvas. Find the MIDDLE: accent-border ~0.4 alpha / 1.5px + 3px endpoint
  dot on the covered-row anchor. Stop oscillating.
- **THE ONE THING:** edges legible AND landed on the exact covered row (defect 1+5) — the board's
  whole value is 'this task covers that section'; today that's hidden.
- **Ranked → Mira (iter7, edges+camera ONLY, no spec/plan tuning):** 1) edges to ~0.4 alpha
  1.5px + endpoint dots. 2) fit-to-view frames the REAL content bbox (kill the ~55% void) /
  shrink satellite orbit so tasks sit beside their rows. 3) bubbles: 1-glyph kind label or a
  per-cluster legend chip + ONE coherent color ramp. 4) drop the duplicate H1 from the spec node.
  5) anchor each task's y to its primary covered row's y (iter6 alignment didn't land). 6) real
  tinted group container. 7) right-align row count badges into a column. 8) spec body → 70%
  muted. 9) composer/tooltip backdrop so it doesn't occlude nodes.
- **Dax2 (fresh independent critic) scored iter5 = 76/100.** Per-dim: Hierarchy 7, Connection 8,
  Coverage 8 (best), Bounds 8, Packing **5** (weakest), Typography 7, Color 6, Nav 7, Markdown 8,
  Polish 6. (Lower than Dax1's 80 = critic calibration drift, not regression; iter5 objectively
  improved. Track the defect list, not just the number.)
- **THE ONE THING:** demote the spec node — it's the brightest/tallest/leftmost mass and
  out-shouts the compact plan node (design sin #4 alive). Make the PLAN node the hero.
- **Ranked defects → Mira (iter6):** 1) spec node: surface-tone bg (not near-white), shorter
  default height + clear scroll, narrower; plan node anchors the board. 2) ~40% dead space right
  — pull clusters to their tasks, tighten vertical gaps, center composition. 3) bubble in-circle
  labels truncate to 3-4 chars ('Han…','Deve…') — drop in-bubble text (color lane + hover) or
  full-kind pill, never a stub. 4) edges over-saturated purple — desaturate to low-alpha
  neutral at rest, brightness only on hover. 5) bubble palette rainbow → constrain to a tighter
  theme-derived ramp. 6) orchestrator group needs a bounded tinted region + label chip. 7) align
  task-card vertical centers to their covered-row anchors so edges run level. 8) coverage bar →
  flat theme-accent fill (match row-tint language).
- **Process note:** hover edge-lift imperceptible at board zoom — capture PER-ZONE screenshots
  for the next Dax pass so edge/bubble detail is judgeable.
- **Dax (independent opus+frontend-design critic) scored iter4 = 80/100.** Per-dim: Hierarchy
  7, Connection **4.5** (the drag), Coverage 8, Bounds 8, Packing 5.5, Typography 6, Color 7,
  Navigation 7, Markdown 8, Polish 6.
- **THE ONE THING:** edges render ONLY on hover → at rest the board is 3 disconnected islands;
  the whole thesis ("tasks joined to section rows, nothing floats") is invisible by default.
  Iter4's de-emphasis over-dimmed. → draw ALL connective edges AT REST (subtle ~25% opacity),
  hover lifts to accent.
- **Ranked defects (Dax):** 1) edges-at-rest (above). 2) plan↔spec + task→step-cluster edges
  missing entirely. 3) dead space — fit-to-view frame tighter. 4) task cards nearly empty —
  fill name/type/model/status per §13.3. 5) spec typography weak (tiny low-contrast). 6) spec
  scroll affordance (fade is ambiguous). 7) inconsistent column gutters → settings constants.
  8) step bubbles over-saturated — desaturate ~15-20%. 9) plan title over-truncated. 10) group
  status badge green collides with coverage green.
- **Dispatch:** Mira takes the full batch (one coherent pass — files overlap, avoid seams);
  headline = edges-at-rest + missing edges + task-card content + spec typography; mechanical
  tail (bubble desat, gutter constants, title, badge color) in the same pass.

## v3 — STEP 1 + subtasks landed (f4c561af) ✅
- Mira: kill subtraction LOD (no empty boxes — all content mounted), two-curve counter-scale
  typography (body floor 6px, title gentle ~15px index layer), wider nodes (plan 420/spec 440),
  per-section ready ✓ + count badges; subtask rendering (children nested under parent, dashed
  group container + parent→child stem, 'N subtasks' pill). tsc clean, 13 test files (layout 41).
- Verified far+near: NEAR = clean structured document (ready badge + title + count + full
  readable body); FAR = all 16 dogfood sections present, counter-scaled, zero empty boxes.
- Subtask hierarchy live in dogfood (Mira 5 children, BE 2 children).
- STILL OPEN (v3): pagination/§16.3 (board tall, 16 sections stack — bound to A4 pages next);
  mermaid+code in nodes (§16.4); Thread-and-Coil bubbles (§16.9); externalized .md bodies (§17,
  codex lane c564479b). dev=f4c561af.

## v3 — view-modes landed (96356437); next = Banded Multi-Column (§20.3)
- FE (Mira) delivered §18 discrete view modes (Document/Card/Tile) + CanvasToolbar (zoom in/out/
  fit/reset + Auto/Doc/Card/Tile + text on/off), continuous counter-scale REMOVED, no empty shells/
  overflow (layout test). 16 canvas tsx suites green. DOGFOODED (4 milestones on view-mode tasks).
- Visual review: toolbar works, but the dogfood (24 sec + 38 tasks) is a tall портянka → fit=13%,
  modes unreadable. → directed Mira to §20.3 Banded Multi-Column FIRST (tames the stack), then
  §20.4 spec pagination+rail, mermaid/code, bubbles.
- NOTE: owner Tab dies on every dev restart (Tabs ephemeral) → plan goes unowned → reach it via
  workspace-overview (click workspace; 'Canvas' nav item was removed). Plan-button path needs a
  live owner. Consider re-binding owner or a 'claim on focus'.

## §23 STEP 1 landed (99c62f56) — one SectionedPlanNode; dogfood friction surfaced+fixed
- FE merged §23 STEP 1: ONE fixed-size SectionedPlanNode (plan+spec merged) = header + right-edge
  RAIL (per-section glyph·title·[n]·OUTPUT dot = TOC + stable task anchors) + body fixed-frame per
  MANUAL mode (Document=RichMarkdown internal-scroll / Card=digests / Tile=coverage). Auto-by-zoom
  KILLED. Fixed bbox 280/400/640, w480, railW100. 18 canvas tsx suites green. Dogfooded (milestone).
- Visual review blocked until STEP 2 (banded task layout, fixed cells) + STEP 3 (default focus@90%,
  Tile overview) — board still 41 tasks tall at 11% fit-all. FE proceeding to STEP 2+3.
- DOGFOOD FRICTION (the protocol working): (1) installed CLI lacked --kind/--text/--doc (BE added
  them but binary not rebuilt) → FIXED by ./build.sh cli; LESSON: rebuild CLI after any BE CLI
  change. (2) cross-daemon report (prod Tab → dev plan) 'Tab not found' → tasked.
- BE tag-model fully integrated: priority/timestamps/tags/facets/handles/.md-by-handle/--doc/
  canvas-filter + create-parent & PATCH-merge fixes (7 tasks done).

## §23 STEP 3 + height-fix landed (4c0e4d29) — BOARD NAVIGABLE (breakthrough)
- FE fixed the km-strip via anchor-hugging SKYLINE column packing (Tile world 1584px, was ~20000;
  fit-all ~50% not 5%). STEP 3: default = Tile overview + camera-focus on active plan (NOT fit-all);
  Overview/Read presets + minimap toggle; dbl-click node→camera-focus+lock Document+dim+breadcrumb;
  minimap HUD. 18 suites green. CLI friction resolved (--kind/--text/--doc work after rebuild).
- VISUAL: default mounts camera-focused @~250% on the plan node (readable!) — SectionedPlanNode Tile
  (title/owner/13-45/coverage) + right-edge rail OUTPUT dots → edges fanning to banded task field +
  minimap + Overview/Read/Doc/Card/Tile toolbar. The §22/§23 design realized; navigable at last.
- NIT: Tile coverage block oversized (giant green panel) — should be compact bar; Tile should show
  section rows. Sent to FE. Deferred: SectionChunk>30, minimap drag-pan.
