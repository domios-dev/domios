---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_9d63ab9e390845e9b790d65048d07222
handle: coverage-model
title: Coverage Model
---

**HOW coverage is derived (never stored).** Storing coverage invites drift; instead `src-tauri/src/plan/coverage.rs::compute_coverage` recomputes it on every read by walking the section tree (subsections included) and tallying each task's `section_ids` by status:

```rust
pub fn compute_coverage(plan: &Plan) -> Coverage {
    let mut ids = vec![]; walk(&plan.sections, &mut ids);   // recurse subsections
    let mut per: HashMap<String, SectionCoverage> = HashMap::new();
    for id in &ids { per.entry((*id).into()).or_default(); }
    for t in &plan.tasks {
        for sid in &t.section_ids {
            let e = per.entry(sid.clone()).or_default();
            e.total += 1;
            match t.status { TaskStatus::Done => e.done += 1, TaskStatus::InProgress => e.in_progress += 1, _ => {} }
        }
    }
    let covered = ids.iter().filter(|id| per.get(**id).map_or(false, |c| c.total > 0)).count();
    let plan_covered_fraction = if ids.is_empty() { 0.0 } else { covered as f64 / ids.len() as f64 };
    Coverage { per_section: per, plan_covered_fraction }
}
```

The per-section `{total, done, in_progress}` rollup drives the canvas treatment: covered rows get a fill tint, **uncovered rows get the amber/dashed treatment so gaps pop**, and the footer reads `done/total · NN% covered`. The same numbers feed `CoverageReport` (with `gaps` = zero-task sections and `unlinked_tasks`) surfaced on the CLI as `plan coverage` / `plan gaps`, and a `SpecCoverageReport` mirrors it on the spec side via `linked_plan_sections`.

## Key decisions
- **Derived, O(sections + Σ section_ids)** — cheap enough to recompute per snapshot; impossible to desync from the task graph.
- **Tree walk, not flat list** — subsections count toward their own coverage, so a parent group with detailed children reports honestly.
- **Gaps as first-class output** — `plan gaps` makes "what's uncovered" a queryable, advisory next-action, not a thing you eyeball on the board.
