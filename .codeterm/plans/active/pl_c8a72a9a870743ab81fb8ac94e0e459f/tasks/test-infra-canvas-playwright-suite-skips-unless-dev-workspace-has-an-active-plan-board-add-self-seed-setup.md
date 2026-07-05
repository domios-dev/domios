---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_1f36a3e7f9194bd1b11f2414ab501b5f
handle: test-infra-canvas-playwright-suite-skips-unless-dev-workspace-has-an-active-plan-board-add-self-seed-setup
title: 'TEST INFRA: canvas Playwright suite skips unless dev workspace has an active plan board — add self-seed setup'
status: done
tags:
- friction
---

ALL 13-canvas specs (canvas-worlds-v3, substrate-v2-crisp, etc.) skip with 'no active plans in the dev workspace' when a fresh Playwright session lands on an empty board. They depend on the dev workspace already having a plan board open — there's no test-setup that seeds/opens one. This blocks autonomous canvas verification (e.g. substrate-v2 size-constant + crisp eval) without intruding on a shared dev's workspace. Add a beforeAll/helper that creates-or-opens a known plan board (deterministic) so canvas specs are self-sufficient and don't skip.
