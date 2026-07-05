---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_1ba99733f6b84fdbb6c67fb4ce9033f8
handle: milestone-regression-gates-full-suite-once-per-group-close-ms_1ba99733f6b84fdbb6c67fb4ce9033f8
title: BOUNCE
---

milestone-regression-gates f0cd51fd reviewed; new PlanError::GroupNotFound was not threaded through REST error mappers or CLI plan_errors renderer, and new comments violate comment-hygiene. Worker instructed to update all PlanError matches, add group_not_found renderer/tests, trim comments, and re-report.
