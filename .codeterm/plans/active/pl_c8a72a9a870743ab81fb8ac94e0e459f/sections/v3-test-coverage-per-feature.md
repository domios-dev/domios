---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: orphan_markdown
id: sections/v3-test-coverage-per-feature.md
handle: v3-test-coverage-per-feature
title: sections/v3-test-coverage-per-feature.md
---
Standing rule: EVERY implementation task ships its own tests (tsx happy-dom for logic + a Playwright spec that runs against dev for visual/no-overflow) and the agent self-checks via Playwright before reporting. Dedicated specs grow the canvas-layout-integrity suite. Orchestrator runs the far/mid/near + per-view-mode + per-facet visual loop and opus+frontend-design Dax reviews.
