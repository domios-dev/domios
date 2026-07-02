---
plan_id: pl_a022a22d8f4e42d983cca288bd264e43
kind: task
id: task_f1f5d9ff72ee44c4889caf2f58c431ff
handle: plan-create-from-spec-id-store-create-plan-from-spec
title: plan create --from-spec <id> + store create_plan_from_spec
status: done
---

Add codeterm plan create --from-spec <spec_id>: store create_plan_from_spec creates a Plan with primary_spec_id set, scaffolds plan sections mirroring the spec section tree, and auto-creates spec_links (spec_section->plan_section). Remove or repurpose the --spec/--spec-file text shortcut so it never dumps prose into a plan section.
