---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_79d5944f871446718bd4909c4f3c7db5
handle: banded-multi-column-task-layout-20-3-ms_79d5944f871446718bd4909c4f3c7db5
title: review
---

STEP 2 review (orchestrator): banded produces 3 cols + step text in Document + scroll-gating, tests green. BUT world still ~20000px tall → fit-all 5%, not visually reviewable. Likely bug: plan-node Document height not truly fixed at 640 OR task field spans huge Y range instead of hugging fixed rail outputs. Directed FE: (a) bound world height + add Playwright height assert, (b) STEP 3 default-focus@90% (prereq for any readable view). Then real visual review.
