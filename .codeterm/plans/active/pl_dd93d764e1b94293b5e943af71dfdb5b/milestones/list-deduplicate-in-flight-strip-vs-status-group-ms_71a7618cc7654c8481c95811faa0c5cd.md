---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_71a7618cc7654c8481c95811faa0c5cd
handle: list-deduplicate-in-flight-strip-vs-status-group-ms_71a7618cc7654c8481c95811faa0c5cd
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: activeWorkStripVisibility.ts (new predicate), activeWorkStripVisibility.test.ts, PlanListView.tsx (wire stripEntries), PlanListView.test.tsx (+4 integration tests). SHA 4afba01e on lane/list-inflight-dedupe pushed to origin.
BUILD/TYPECHECK/TESTS: npx tsx activeWorkStripVisibility.test.ts PASS; npx tsx PlanListView.test.tsx 18/18 PASS; npx tsx ActiveWorkStrip.test.tsx 2/2 PASS; git diff --check clean. No cargo/build/Playwright (per weak-machine lane).
FOLLOWUP: Orchestrator should verify in UI on dev after merge. Predicate: shouldShowActiveWorkStrip + activeWorkEntriesForStrip — strip hidden when all in-flight tasks visible in list; strip shows only XOR-hidden entries when filters/search/collapse hide any in-flight task.
