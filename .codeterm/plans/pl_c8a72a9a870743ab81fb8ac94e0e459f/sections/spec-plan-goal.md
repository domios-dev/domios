**HOW Spec and Plan stay orthogonal but coupled.** Spec = WHAT, Plan = HOW; they are the *same node type* on the canvas, joined by coverage. The implementation has three moving parts: coverage is **derived, never stored**; spec↔plan is **one sectioned-coverage node kind**; and unlinked tasks get auto-relinked by a structural heuristic.

## 1. Coverage is computed on read, never persisted

Storing coverage would let it drift from the tasks. Instead `compute_coverage` walks the section tree, tallies tasks per section by status, and derives the plan-level fraction as `covered_sections / total_sections`. It's recomputed every snapshot — cheap, always correct:

```rust
pub fn compute_coverage(plan: &Plan) -> Coverage {
    let mut ids = vec![];
    walk(&plan.sections, &mut ids);          // recurse subsections too
    let mut per: HashMap<String, SectionCoverage> = HashMap::new();
    for id in &ids { per.entry((*id).into()).or_default(); }
    for t in &plan.tasks {
        for sid in &t.section_ids {
            let e = per.entry(sid.clone()).or_default();
            e.total += 1;
            match t.status {
                TaskStatus::Done => e.done += 1,
                TaskStatus::InProgress => e.in_progress += 1,
                _ => {}
            }
        }
    }
    let covered = ids.iter()
        .filter(|id| per.get(**id).map_or(false, |c| c.total > 0))
        .count();
    let plan_covered_fraction =
        if ids.is_empty() { 0.0 } else { covered as f64 / ids.len() as f64 };
    Coverage { per_section: per, plan_covered_fraction }
}
```

The status rollup per section (none/pending/in_progress/done) becomes the canvas fill tint — covered rows get a tint, **uncovered rows get the strong amber/dashed treatment so gaps pop**. The same numbers surface on the CLI via `plan coverage` / `plan gaps`, and a spec-side `SpecCoverageReport` mirrors it block-to-block (which plan sections a spec block links to).

## 2. Plan and Spec are one sectioned node kind

Rather than a reduced "spec card", both render through the same sectioned-coverage node: section TITLE *and* full markdown body (mermaid + code highlighting reuse the chat `RichMarkdown` renderer). That's why this very plan carries its spec sections (Foundation, Concept Vision, Data Model…) inline as the upper sections and its HOW sections (Substrate, Infra, Tests) below — one tree, one renderer, one coverage model. You fly/zoom to read spec, plan, and tasks at any depth; the LOD bands (W0→W3) decide how much body shows.

## 3. Auto-relink: structural, not manual

16 unlinked tasks shouldn't have to be hand-wired to sections. `section_links.rs` builds normalized key sets per section title and matches task names against them, returning a `SectionRelinkReport` (preview vs `--apply`) so a task drops onto the section it actually covers:

```rust
fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace().collect::<Vec<_>>().join(" ")
}
```

Surfaced as `plan task relink <plan>` (preview) → `--apply`. Coverage then recomputes for free, since it's derived.

## 4. Provenance flows through the system

Brainstorms/decisions/discussions aren't lost to chat — they're externalized `.md` attached to the relevant task/milestone (same body-sidecar mechanism as Infra §), reachable via `agent report --plan --task --doc <md>` and rendered expandably on the canvas. The strides recorded on this task (`plan record … --step decision:…`) are the same idea dogfooded: the plan documents its own construction.

## Key decisions / tradeoffs
- **Derived coverage over stored** — impossible to desync from tasks; cost is recompute per snapshot (negligible at plan scale; the tree walk is O(sections + task·section_ids)).
- **One node type over a spec/plan dichotomy** — a single renderer + coverage model, no parallel reduced variant to keep in lockstep; cost is that spec and plan share LOD/sizing rules and must both look right at every band.
- **Heuristic relink as advisory** — preview-then-apply keeps a wrong title-match from silently mis-covering a section; the human/orchestrator confirms `--apply`.
