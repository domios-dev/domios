---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_d440881e98b0426388ae6c6cc3ecefeb
handle: extract-shared-activetabpill-dedupe-tabitem-workspaceplanentry
title: Extract shared ActiveTabPill (dedupe TabItem/WorkspacePlanEntry)
status: done
---

Reviewer DRY finding: the active-pill div is duplicated verbatim between TabItem.tsx (~200-207) and WorkspacePlanEntry.tsx. Extract a shared ActiveTabPill component; both consumers use it; no visual change (screenshot-compare before/after).
