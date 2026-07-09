---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_3e9478d68bf443279d5316db8f762c32
handle: f1-plan-recall-index-mem-sidecar
title: F1 Plan Recall Index (mem sidecar)
status: done
---

# F1 Plan Recall — implementation spec (UNIFIED into mem search + plan related)

## GOAL — make `codeterm mem search` ALSO find plan/spec/task/stride content (UNIFIED search, no new search verb), + a structural `plan related`. The plan index lives IN-PROJECT.

### A. Project-local mem db (EXTEND the mem store)
- Extend the mem store so, in addition to the existing GLOBAL db, it has a PROJECT-LOCAL db under the project's `.codeterm/` (e.g. `.codeterm/mem-plan.sqlite`), gitignored. Reuse the SAME mem schema + engine (FTS5 + sqlite-vec + fastembed + RRF) — it's just a second db location.
- Plan/spec units are mirrored into THIS project-local db: kinds `plan | plan_section | spec | spec_section | task | stride`, embed-only (skip the LLM extraction/graph/dreaming entirely — §21), scope = `plan_<hash(canonicalized project_root)>` (stable across workspace churn).

### B. Mirror plan/spec → project-local mem (unit = chunk, enriched text)
- One mem item per unit. Enriched indexed text carries context (Andrey): spec section → `Spec "<spec title>" · section <N>/<total>: <title>\n<body>`; plan section → `Plan "<plan title>" · section <N>/<total>: <title>\n<body>`; task → `Plan "<plan title>" · task: <name>\n<body>`; stride → `<parent label> · stride(<kind>): <text>`. Stable id `planunit_<plan_or_spec_id>_<kind>_<unit_id>`. Long bodies (>~512 tok) split by markdown heading/paragraph, each its own vector, same back-pointer.
- UPDATE: on daemon plan/spec write (mutation/batch apply) the mirror upserts changed units (FTS immediately, vector on the existing async embed queue, OFF the write path). On direct disk edit, the EXISTING plan file-watcher reload calls the mirror. UPSERT by stable id (old vector replaced — NO orphans). Unit removed / plan|spec archived → delete its rows (same txn). Backfill existing plans/specs on first search or `mem search --reindex` / a `plan reindex`.

### C. `codeterm mem search` — EXTEND, do not fork
- When run in / scoped to a project, `mem search "<q>"` ALSO queries that project's project-local plan db and FUSES (RRF) with global-mem results, TAGGING each hit by source: `[mem]` vs `[plan]`. Reuse mem's existing filters (--kind, --project, --limit, --scope); add the plan unit kinds to --kind. Human output keeps mem's format + a source tag + (for plan hits) plan/spec title › unit title · status. --json adds `source` + plan back-pointer fields. NO new `plan search` / `plan similar` commands.
- Project resolution: default from CWD (git toplevel / nearest .codeterm); `--project <path>` to override; not in a project → just search global mem (no error).

### D. `codeterm plan related --id <unit> [--depth N] [--json]` (structural — keep in plan ns)
- Traverse the plan/spec OWN edges (NOT a mem graph): task.section_ids ↔ sections, parent_task_id subtasks, spec_links (spec_section ↔ plan_section), effort lineage. Return connected units grouped by edge type. MAY add one vector-neighbor line via the mem engine (no standalone `similar` command). DANGLING-SAFE: an edge to a removed/archived target prints `⊘ removed` and is skipped — NEVER panic/error (required test).
- REST + socket mirror; regenerate docs/cli-manifest.json + docs/CLI.md.

### E. TESTS (TDD, write first — cover ALL)
- mirror upsert on create; reindex on edit (old gone, new present); delete on unit-removal + plan/spec archive → ZERO orphan vectors; scope stable across workspace churn; embed-only (assert NO extraction/graph worker invoked); recall (seed known text → `mem search` returns the unit ranked, tagged [plan]); related traversal returns expected units THEN skips a deleted/archived target without crashing (dangling-safety — required); RRF fusion of [mem]+[plan] results.
- ACCEPTANCE: `CODETERM_API_PORT=7694 codeterm mem search "plan button soft-lock"` (seeded plan in your instance) returns the soft-lock task tagged [plan].

GUARDS: index is DERIVED (plans/specs JSON authoritative); no extraction/graph/dreaming over plan units; keep the mirror in its own module (service/plan/search_index.rs) hooked at ONE write-path call site; the project-local-db change lives cleanly in mem/* . English; files <1000 lines; cargo fmt+clippy+plan/task/spec/mem tests green before push; `CODETERM_API_PORT=7694 ./build.sh cli` after CLI changes.

DOGFOOD: plan pl_c8a72a9a870743ab81fb8ac94e0e459f, task F1 task_3e9478d68bf443279d5316db8f762c32 (subtasks: f1-index-store, f1-unit-mirror, f1-revector, f1-cli, f1-tdd-tests). Report strides via `CODETERM_API_PORT=7686 codeterm plan record pl_c8a72a9a870743ab81fb8ac94e0e459f --task <subtask_id> --step "decision|testing:..." --json` AND to me (parent 073f64ae). Friction = a bug we fix. (The real dogfood-plan backfill + recall demo on 7686 is run by the orchestrator at integration.)
