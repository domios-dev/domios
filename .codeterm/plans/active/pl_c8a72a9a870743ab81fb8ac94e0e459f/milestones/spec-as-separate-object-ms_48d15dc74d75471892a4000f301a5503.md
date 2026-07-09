---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_48d15dc74d75471892a4000f301a5503
handle: spec-as-separate-object-ms_48d15dc74d75471892a4000f301a5503
title: decision
---

SPEC-OBJECT synth (codex 21ea5411, 1/2). Spec = first-class SIBLING JSON .codeterm/specs/sp_<uuid>.json (single-writer+watched+externalized .md+INDEX.md). Spec{id,title,project_path,status,sections[SpecSection prose],links,canvas}. PlanSection becomes work-breakdown (no longer spec source); tasks keep Task.section_ids vs plan section ids. Link = SpecPlanLink{spec_id,spec_section_id,plan_id,plan_section_id,kind:satisfies|implements|decomposes|related} stored canonically in Plan.spec_links + mem-indexed by spec_id; Plan.primary_spec_id optional/UI-only; n:m both ways.
