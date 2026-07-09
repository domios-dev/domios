---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_1555ba110d9045c7b27465a3ba95e446
handle: ui-ux-iter20-final-review-perf-re-baseline-20k-emergence-integration-ship-ready
title: 'UI/UX iter20 — FINAL review: perf re-baseline @20k + emergence integration (SHIP-READY)'
status: done
---

Perf @20k WITH emergence active: 35.5fps median (glide ON==OFF -> emergence adds ~0 steady-state cost; the 46->36 gap is web-Chromium env + richer field-coverage cards, NOT emergence). Bounded: pins peaked 37/48, faces 5/32, 54 task cards in DOM at 20k (virtualized). Layout/scene memos cameraScale-decoupled (static+dynamic). Integration: ALL emergence features coexist, no z-index/click conflicts (presence container pointer-events:none, cluster clears toolbar). 0 overflow + W0-meaningful across EVERY type incl handoff/note/decision/ghost/group (new final-types-audit). tsc+build green; whole canvas unit suite GREEN (orchestrator fixed the last 3 stale tests: §34, spec-block, crispRasterScale, §35 comet-fold). file-sizes: no canvas file >1000. VERDICT: SHIP-READY. Committed feat/canvas-perf-polish (unpushed). DATA DEPS for daemon owner: live pane/agent-registry->buildTaskPresence; orchestrator heartbeat.current_focus/active_lanes; plan-index done_count; .codeterm/plans indexing.
