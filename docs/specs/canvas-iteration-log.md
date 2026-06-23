# Canvas v2 — Iteration Log (durable loop state)

Append one block per iteration. On context loss, re-read this to resume the loop.
Canonical dogfood plan on dev: **`pl_48a4ee030d934f4298fab76fe353fedc`** (project
`/Users/rollacode/Developer/codeterm`). Builder branch: `feat/canvas-redesign`.
Integration branch: `feat/canvas`. Loop plan: `2026-06-24-canvas-iteration-loop.md`.

---

## Iteration 0 — structural v2 base (in progress)
- **Builder:** FE lane, pane `3c68143d` (claude opus, frontend-design skill).
- **BE lane:** pane `b1583905` (codex) — standalone tasks/Inbox, task CLI, canvas focus,
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
- **Polish defect backlog → Mira (FE pane 3c68143d):**
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

## Iteration 6 — Dax re-score + final polish — in progress
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
