---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_665e61df45564dba955cf2dea7238766
handle: fix-single-camera-transform-remove-crisp-zoom-wrapper-cloud-flush-overflow-polish
title: FIX — single camera transform (remove crisp zoom-wrapper) + cloud flush + overflow polish
status: done
---

Removed the per-card .ct-canvas-crisp zoom+counter-scale wrapper (TaskCard/SectionNode) so cards render under the SINGLE camera transform - directly fulfills this plan's goal vector (object never changes size, camera-only zoom, zero reflow). Fixes deep-zoom cramping/clipping: title/body wrap INSIDE bounds, milestone bubbles inside as rows, no mid-word truncation. Also: #1 task-cloud flush to plan (gap 173px->36px, 0 overlap, guarded by cloudFlush.test.ts 9/9); focus-framing padding (focused card fully in viewport); 2-line title wrap. VALIDATED against THIS real plan: spec-left / plan / tasks-right composition correct, cards render clean. Lane cm-canvas-fix, tsc green, uncommitted (awaiting Andrey's commit).
