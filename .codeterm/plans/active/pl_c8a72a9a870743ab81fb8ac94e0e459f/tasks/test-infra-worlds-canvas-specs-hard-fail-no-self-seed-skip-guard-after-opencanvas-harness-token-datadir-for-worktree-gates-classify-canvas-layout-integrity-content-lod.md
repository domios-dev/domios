---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_84abc9bd117a4da381ddec71c3b40852
handle: test-infra-worlds-canvas-specs-hard-fail-no-self-seed-skip-guard-after-opencanvas-harness-token-datadir-for-worktree-gates-classify-canvas-layout-integrity-content-lod
title: 'Test-infra: worlds canvas specs hard-fail (no self-seed, skip-guard after openCanvas) + harness token/datadir for worktree gates + classify canvas-layout-integrity content-LOD'
status: done
tags:
- state-integrity
---

Combined integration gate (Lamarr, canvas/orch-integrated) found: (1) canvas-worlds/-v2/-v3 specs take only {ui}, seed nothing, and their isEmpty skip-guard sits AFTER openCanvas -> on a fresh instance openCanvas hard-times-out (no plan pane) and the test HARD-FAILS instead of skipping. Fix: move empty/skip check before/into openCanvas OR seed in beforeAll (reuse seed-canvas.ts). (2) Harness readDevToken() defaults to ~/.codeterm-dev -> wrong token for an isolated worktree instance (90s waitForShell timeouts); workaround CT_E2E_DATADIR=<gate datadir>; fix: auto-derive datadir from CT_E2E_PORT. (3) canvas-layout-integrity: canvas renders (nodes count=1) but post-setCanvasScale(1.6) the content-LOD reveal [canvas-spec-section-body] never appears in 30s — classify: real content-LOD regression vs setCanvasScale ineffective headless (baseline on the lane branch). Substrate-v2 + seed-smoke + layout render all PASS, so canvas DOES render in web-mode (not a web-mode incapability).
