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

## Iteration 1 — first v2 capture & review (next)
- After dev is healthy on 8a548a83: Playwright screenshot the dogfood board (Plan mode) →
  view → score vs rubric → log defects → critique to Mira / spawn Dax. Verify: no-drag,
  coverage tint, edges connect, bubbles cluster, markdown renders, nothing overflows,
  isolation (plan absent in other workspace).
