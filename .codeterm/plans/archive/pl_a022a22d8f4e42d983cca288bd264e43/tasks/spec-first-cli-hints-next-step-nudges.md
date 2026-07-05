---
plan_id: pl_a022a22d8f4e42d983cca288bd264e43
kind: task
id: task_13b3072db79049d2836a2282005878e6
handle: spec-first-cli-hints-next-step-nudges
title: Spec-first CLI hints (next-step nudges)
status: done
---

Emit hints: spec create -> add sections then plan create --from-spec; spec section add -> link or build plan; plan create WITHOUT a spec -> warn plans are built from a spec; after plan sections exist with a bound spec but no links -> nudge spec link add. Reuse cli/src/cmd/hints.rs.
