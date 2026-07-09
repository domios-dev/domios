---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_e8d5f4e1c4f8486b9467591329fc57c6
handle: s1-spike-minimal-substrate-board-task-cards-zoom-crisp-focus-by-id-ms_e8d5f4e1c4f8486b9467591329fc57c6
title: testing
---

M0+M1 CODE accepted (orchestrator diff-review + tsc). substrateFlag.ts (localStorage canvas.substrateV2, default OFF, SSR-safe) + TaskCard early-return: flag ON -> card(style) pure DOM no crispScale wrapper, flag OFF -> byte-identical old behavior (ships green). Clean boundary: TaskCard.tsx + new file only, supervisor's CanvasView/SectionNode untouched. @e5bc9522 on canvas/mig-m1-taskcard. PENDING for S1 completion: in-app Playwright (size-constant W1->W3 + crisp@1000% + frontend-design eval) — needs dev redeploy with flag ON.
