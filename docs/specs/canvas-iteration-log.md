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

## Iteration 2 — fix rescan (Rex) + polish pass 1 (Mira) — in progress
