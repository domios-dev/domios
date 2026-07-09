---
plan_id: pl_a022a22d8f4e42d983cca288bd264e43
kind: task
id: task_35181929d2f34e5da21d67469bfa4742
handle: canvas-spec-plan-m-n-edges-from-spec-links-spec-node-sizing
title: 'Canvas spec⊥plan: M:N edges from spec_links + spec-node sizing'
status: cancelled
---

Spec node now renders real spec.sections (done). Remaining: the spec-row edges still assume 1:1 same-section (intraPlan.ts); draw them from the real plan.spec_links (spec_section -> plan_section, M:N). Size/position the spec node by its OWN section count, not the plan flat length.
