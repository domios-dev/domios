---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_0d3bcacbed59448ebc211943bac90953
handle: t7-review-ms_0d3bcacbed59448ebc211943bac90953
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: none (review-only lane; no commits on lane/t7-review)

VERDICT: BOUNCE — 3 of 5 command gates red at 40cdddd6. Concrete bounce list at bottom.

== FINDINGS (by severity) ==

F1 HIGH build+runtime — 40cdddd6 imports CANVAS_MIN_SCALE/CANVAS_MAX_SCALE with 'import type' (useCanvasFlyTo.ts:2) but uses them as VALUES at useCanvasFlyTo.ts:112. tsc TS1361 x2 -> 'npm run build' (tsc && vite build) FAILS; in dev/esbuild the type import is erased so centerCameraOnNodes throws ReferenceError whenever a canvas-focus event carries 'scale' (backend go-to-id-@-zoom nav, emitters src-tauri task_routes.rs / socket/server/canvas.rs; CanvasView.tsx:500 passes p.scale). Base had numeric literals 0.05/10; v3 swapped to constants without fixing the import. Owner: t11-canvas-v2 (zone-first v3 commit).

F2 HIGH unit gate RED — 3/131 test files fail (full suite run locally): (1) layout/cloudFlush.test.ts 3 asserts 'nearest task flush at planRight+columnGapX gap=50 expected 36'; (2) layout/layoutInputRevision.test.ts 'layout bounds grow when tasks are added'; both pre-existing tests broken by v3 geometry in 40cdddd6 — the accept gate's focused list (canvasFit/zoneGrid/layout/...) did not include these two files; (3) planView/PlanDetailOverlay.test.tsx 'focus trap wraps Tab within panel' — trap logic PlanDetailOverlay.tsx:67-83 vs DetailContent focusable order. Owners: t11 (1,2), t2-detail-overlay/t2b-spec-pres (3).

F3 HIGH file-size gate RED — scripts/check-file-sizes.sh: 4 non-allowlisted files >1000, ALL regressed in-range (base->now): packages/chat-ui/src/MessageBubble.tsx 995->1020 (3e4972e0), src-tauri/src/service/plan/mod.rs 917->1014 + src-tauri/src/socket/server/plan.rs 989->1041 (2e58df9a/d790fa4b, t0d-id-ergonomics), src-tauri/src/service/plan/tests.rs 952->1009 (0e9c1c68, t0b-role-field). mod.rs also violates the <800 facade advisory.

F4 MED perf churn — PlanBoardView.tsx:200-216 and PlanListView.tsx:97-113 spec-section fetch effects depend on selectedPlan OBJECT identity; usePlans.ts:116 refetches plans on every plan-changed event, so during active orchestration each stride/heartbeat mutation triggers a fetchSpecSections GET per mounted view. Not per-render, but unbounded per-event refetch of rarely-changing data. Fix: key deps on selectedPlan?.id + primary_spec_id and/or skip when already fetched. Owner: t9-board-v2/t4-list or fold into t16-list-v4.

F5 MED spec gate 3 unproducible — no frame-budget evidence exists for canvas v3: t5-canvas report explicitly deferred it ('Frame probe before/after needs dev+fixture'); canvas-perf.spec.ts is full-project-only (not @golden); and my run at HEAD FAILS before measuring: ensureCanvasBoard (helpers/canvas-inspect.ts:326) waits for [data-testid=canvas-view] but board-first default lands on Board and the helper never switches mode — canvas-perf.spec.ts and canvas-ux1-schematic.spec.ts are both broken by t1's default change. Owner: t6-e2e-gate (helper fix + produce frame numbers).

F6 LOW-MED UX flag — max zoom collapsed 10x->4x (CanvasView.tsx:196-199 now uses settings.ts:191 zoomMaxScale=4; deleted inline comment justified 10x for worlds-dive stride reading). Deep zoom-drill legibility needs explicit visual check in round-4 screenshots. Owner: t11/planner.

F7 LOW DRY — ~90 lines of plan-selection/sessionStorage/spec-fetch logic duplicated verbatim between PlanBoardView.tsx:160-232 and PlanListView.tsx:56-123; formatRelativeAge duplicated with DIVERGENT behavior (boardModel.ts:98 caps at days; planListModel.ts:25 adds w/mo). Extract usePlanSelection hook during t16-list-v4.

F8 INFO — t6 gate stride records no screenshot artifact paths (spec gate 4 requires paths attached to reports); visual acceptance correctly pending user via loop (2/3 iterations).

== PER-LENS ==
a) untouched flows: presence layer intact (CanvasPresenceLayer untouched; LOD groups consume presence additively). Fly-to: fit path OK, scale path crashes (F1). Composer: legacy strips removed cleanly, new ComposerPlanActivityStrip has testids+tokens. Multi-plan stacking removed deliberately per spec ROUND 2 (single plan + PlanSwitcher) — not a regression.
b) perf: no per-render fetches; all fetches effect-scoped (F4 churn aside); board/list models memoized; LOD memos keyed on throttled transform.scale. Frame budget UNEVIDENCED (F5).
c) discipline: comments clean (zero 3+-line comment blocks added, 1-liners only); zero raw hex added in src diff; globals.css uses var(--ct-*) throughout; file sizes RED (F3).
d) contracts: testids preserved — CanvasDetailOverlay is a 58-line thin adapter passing testIdPrefix='canvas-detail' (CanvasDetailOverlay.tsx:55) so canvas-detail-* hooks survive; plan-board-*/plan-list-*/plan-detail-* present; PlanDetailOverlay props are model-level (Task/Plan/Section/Pane) — only nav-callback arg type SectionNavArgs imports from ../nodeRegistry, acceptable; no LaidOutNode in planView/.

== GATE TABLE ==
lint: PASS — biome check . = 656 files 0 errors (1 pre-existing warning: bundled plugins/git/ui app.js >1MiB). NB: must unlink the warm-checkout node_modules symlink first, biome follows it and dies (see FRICTION).
test:unit: FAIL — 3/131 files (F2), full suite run on this machine.
check-file-sizes.sh: FAIL — 4 files (F3).
playwright golden: PARTIAL PASS — plan-view-mode/plan-detail-overlay/plan-done-heavy re-run LOCALLY vs dev 7686 (health build 40cdddd6 == HEAD): 7 passed 1 known blit-skip in 17.9s, independently confirming orchestrator evidence. Full --project=golden across all suites not run here (orchestrator-owned).
build.sh check: PASS (orchestrator-owned, quoted evidence at 40cdddd6 + pre-push cargo check/fmt/clippy). Not re-run per machine policy.
npm run build (tsc): FAIL — 16 errors: 7 in-scope (F1 x2, layout.test.ts x3, zoneGrid.test.ts x2, all from 40cdddd6); 9x TS2307 @codeterm/plugin-sdk are environmental (fresh worktree lacks workspace install) — verify once in warm checkout.
Spec gates: 1 functional PASS (board default+order+persistence verified by spec run); 2 done-heavy PASS (golden re-run); 3 perf FAIL/unevidenced (F5); 4 visual PENDING USER (loop 2/3, F8); 5 regression FAIL (unit red); 6 code health FAIL (sizes).

== BOUNCE LIST ==
1. t11-canvas-v2 -> in_progress: fix import type -> value import of CANVAS_MIN/MAX_SCALE (F1); fix layout.test.ts TS2353/TS6133 + zoneGrid.test.ts implicit-any; make cloudFlush + layoutInputRevision tests green against v3 geometry (update expectations if v3 is intended).
2. t0d-id-ergonomics + t0b-role-field -> split src-tauri/src/service/plan/mod.rs, socket/server/plan.rs, service/plan/tests.rs under 1000 (mod.rs under 800). MessageBubble.tsx (1020, from 3e4972e0) has no owning task in this plan — orchestrator assigns (integration debt).
3. t2-detail-overlay or t2b-spec-pres -> fix focus-trap wrap failure in PlanDetailOverlay.test.tsx.
4. t6-e2e-gate -> teach ensureCanvasBoard (and any canvas-view-waiting helper) to force mode=canvas under board-first default; then produce gate-3 frame-probe numbers on the 200-task fixture; record screenshot artifact paths in reports.
5. t16-list-v4 (when dispatched) -> absorb F4 spec-refetch keying + F7 dedup.

FRICTION (system feedback): (1) worker-brief advice to symlink warm node_modules breaks 'biome check' — scanner follows the symlink into the other checkout and dies OOM; had to rm/re-ln around lint. (2) rtk output filtering swallowed the failing-file names from scripts/ts-unit-tests.sh (only the '3 of 131 failed' summary survived, no tee log) — cost a full per-file re-run to identify failures. (3) task body says 'codeterm spec show' but natural first guess 'spec get' errors — fine, but plan get --json also buries task handles (had to walk the whole JSON to map handle->id for bounce naming; a 'codeterm plan tasks <plan> --handles' listing would help).

BUILD/TYPECHECK/TESTS: ran locally — biome lint, full ts-unit suite (131 files), check-file-sizes.sh, tsc --noEmit, golden plan-view Playwright group vs dev@HEAD, canvas-perf.spec attempt. Did NOT run cargo/build.sh (orchestrator-owned per machine policy).
FOLLOWUP: dev daemon on 7686 currently runs build 40cdddd6 == HEAD, so UI gates are faithful right now; re-running the golden plan-view group takes ~18s if needed after fixes.
