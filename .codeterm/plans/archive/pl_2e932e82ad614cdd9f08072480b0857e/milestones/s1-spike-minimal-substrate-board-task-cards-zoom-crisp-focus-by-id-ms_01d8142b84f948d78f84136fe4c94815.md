---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_01d8142b84f948d78f84136fe4c94815
handle: s1-spike-minimal-substrate-board-task-cards-zoom-crisp-focus-by-id-ms_01d8142b84f948d78f84136fe4c94815
title: testing
---

LIVE VERIFY ATTEMPTED end-to-end. Applied Brunel's 2 FE files to main checkout (HMR, flag default-off=no functional change, web-mode Chromium=no focus-fight), copied Curie spec, ran CT_E2E_PORT=7686 playwright --grep @canvas-substrate-v2. RESULT: harness CONNECTS+RUNS clean but SKIPS — fresh session canvas board empty (canvas-empty). ROOT CAUSE: pre-existing precondition — the WHOLE canvas suite incl supervisor's canvas-worlds-v3.spec.ts skips identically ('no active plans in the dev workspace'); canvas tests need an active plan board in the workspace, no self-seed setup exists. NOT a M0+M1 or Curie-spec defect. Reverted main checkout to pristine (supervisor tree untouched). M0+M1 ACCEPTED on evidence (S0 real-WebKit crisp proof + diff review + structural size-constancy + tsc). Live size-constant+frontend-design eval gated on a populated+flagged dev (cleanest when M0+M1 merges to mainline) OR canvas-suite self-seeding fix.
