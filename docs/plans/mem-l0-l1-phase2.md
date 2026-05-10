# L0/L1 wake-up redesign — Phase 2

> **Status (2026-05-10):** ready to implement. Design closed in two parallel
> brainstorm tabs (`mem L0/L1 design` gpt-5 + `mem critic tab3` MiniMax) on
> Mac-Air on 2026-05-09; both tabs ran out of token budget mid-discussion
> but landed on the plan below.

## Why this exists

Current `layers.rs` wake-up is too rigid:

- `rule` / `preference` / `peer` items are always L1-injected regardless of
  whether they're useful right now — label spam pollutes the prompt
- `fact` / `summary` only get in if `importance >= 4` — no way to force-pin
  a low-importance item that's still load-bearing for an active workspace
- No per-workspace scoping — when GA is helping with project A, it still
  carries L1 about project B, eating tokens for nothing
- Selection signal is `importance + recent_activity` — `importance` is
  manually set (drift over time, label inflation), `recent_activity` is
  noisy (anything `mem get` touches counts, including dreamer sweeps)

## Phase 1 status (= what we have today, already in `layers.rs`)

| Layer | Source | Budget | Loaded |
|-------|--------|--------|--------|
| L0 — identity | `~/.codeterm/general/identity.md` (text file) | ~50–100 tok | always, in GA system prompt |
| L1 — critical | top-N mem_items, kind-filtered, importance-ranked | ~500–800 tok | always, via `codeterm mem wake-up` |
| L2 — targeted | `mem search --area X --kind Y` | variable | on-demand by GA |
| L3 — hybrid recall | FTS5 + vector RRF-fused | variable | on-demand by GA |

Eviction priority when L1 over budget: `rule > preference > peer > fact > summary`.

This works today but the selection signal is the weak link. Phase 2 fixes
**how** L1 picks what to include without redesigning the layer concept.

## Phase 2 goals

1. **Explicit pinning** — user/agent says "always include this in L1" without
   having to crank importance to 5
2. **Better usage signal** — replace `recent_activity` (polluted by sweeps)
   with `evidence_count` (already in schema, bumped on actual successful agent
   reference per issue #58)
3. **Workspace scoping** — when GA is attached to workspace W, L1 prefers
   items scoped to W over global ones at the margin
4. **Configurable budget** — different machines have different model
   contexts; one hardcoded `l1_budget_tokens` doesn't fit everyone

Non-goals (defer to Phase 3 / out-of-scope):

- LLM-based admission classifier (MemRouter pattern) — research first
- Lane-based selection (separate slots per category) — design after Phase 2
  user feedback
- Learned ranking (XGBoost over features) — needs training data
- `is_starred` (separate from pin) — redundant with pin
- Full L0 dedup against L1 — small cost, user can live with it
- L1 UI redesign in mem-tab — not blocking

## Schema changes

### Two new columns (NOT three)

```sql
-- migration: add to mem_items
ALTER TABLE mem_items ADD COLUMN is_pinned_wakeup INTEGER NOT NULL DEFAULT 0;
ALTER TABLE mem_items ADD COLUMN workspace_id TEXT;

-- index for L1 selection hot path
CREATE INDEX IF NOT EXISTS idx_mem_pinned ON mem_items(is_pinned_wakeup) WHERE is_pinned_wakeup = 1;
CREATE INDEX IF NOT EXISTS idx_mem_workspace ON mem_items(workspace_id) WHERE workspace_id IS NOT NULL;
```

### What we are NOT adding (and why)

- **`cited_count`** — `evidence_count` already exists in
  `src-tauri/src/mem/store/schema.rs` (technically through
  `codeterm_mem_store::graph::bump_evidence_count`, used in
  `auto_extract.rs:339` and `api/mem_routes/items.rs:133`). It bumps when
  a skill/lesson is referenced in a "success-shaped" agent session — this
  is exactly the "uncontaminated usage signal" tab3 asked for. Reuse it
  instead of creating a parallel column. **Resolved open question (1).**
- **`is_starred`** — redundant with `is_pinned_wakeup`. One pin column.

### Migration safety

- New columns have safe defaults (`0` and `NULL`) — existing rows continue
  to work without re-running anything
- Indices are conditional (`WHERE x = 1` / `WHERE x IS NOT NULL`) — small
  size, only the hot rows
- No backfill needed — `evidence_count` already populated by extraction
  worker since v1.1.20

## CLI

### New verbs

```bash
codeterm mem pin <id>                      # set is_pinned_wakeup = 1
codeterm mem unpin <id>                    # set is_pinned_wakeup = 0
codeterm mem pinned [--limit N]            # list currently-pinned items
```

`pin`/`unpin` go through `PATCH /api/mem/items/{id}` with a new optional
`pinned: bool` field — single round-trip, no schema-aware client logic.
`pinned` listing reuses the existing list endpoint with a new `?pinned=true`
filter.

### What `mem save` looks like

`mem save` gets two optional new flags:

```bash
codeterm mem save --kind fact --pin "Always inject in L1"
codeterm mem save --kind note --workspace "$CODETERM_WORKSPACE_ID" "scoped to current"
```

Default = unpinned, global. Backward compatible.

## Config

```toml
[memory]
l1_budget_tokens = 800   # default; was hardcoded
# l0_budget_tokens stays implicit — it's just identity.md size
```

If user doesn't set it, default 800 (current value). One config knob, not
twenty — Phase 3 decides whether to break it into sub-budgets per lane.

## L1 selection algorithm — the new logic

Replace the current `collect_l1_candidates` heuristic with this two-pass:

**Pass 1: include all pinned items (regardless of importance / kind)**
```
SELECT * FROM mem_items
WHERE is_pinned_wakeup = 1
  AND (workspace_id IS NULL OR workspace_id = ?current_ws)
ORDER BY importance DESC, evidence_count DESC, updated_at DESC
LIMIT 50
```

**Pass 2: fill remaining budget by score (skipping already-pinned ids)**
```
score = importance * 1.0 +
        log10(1 + evidence_count) * 0.5 +
        (workspace_id = ?current_ws ? 0.5 : 0.0) +
        recency_bonus(updated_at)        -- decays over 30 days

SELECT * FROM mem_items
WHERE id NOT IN (?pinned_ids)
  AND kind IN ('rule', 'preference', 'peer', 'fact', 'summary', 'skill', 'learning')
  AND (workspace_id IS NULL OR workspace_id = ?current_ws)
ORDER BY <computed score> DESC
LIMIT 200    -- candidate set; render trims by token budget
```

Notes:

- `evidence_count`'s contribution is `log10` so a single use doesn't dominate
  ranking; sustained references compound but with diminishing returns
- workspace match is +0.5 (small thumb on the scale, not a hard filter) so
  global items still surface when they're more important
- pinned items can still be evicted at render time if pinned-set alone
  exceeds budget — but the user gets a `[pinned-eviction]` log line and
  knows to unpin something
- when `current_ws` is `NULL` (GA not in any workspace, or pre-1.0 startup),
  Pass 1's workspace condition collapses to `workspace_id IS NULL` — only
  globally-pinned items load. **Resolved open question (2).**

## Precedence reconciliation

**Resolved open question (3):** is_pinned vs importance:

- `is_pinned_wakeup = 1` is strictly higher priority than any importance
  level. Pinned = "always loaded, full stop."
- Within the pinned set, ties break on `(importance, evidence_count, updated_at)`
- Within the unpinned ranked set, the score formula determines order

Effect: a pinned `importance=2` item beats an unpinned `importance=5`. This
is the desired semantic — "I explicitly told you this one matters" is
always stronger than "the auto-classifier guessed importance".

## Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Schema migration silently fails on Windows-bash | high | guard with `IF NOT EXISTS`; SQLite already idempotent on `ALTER TABLE ADD COLUMN` (newer versions) — fallback: PRAGMA `table_info` check before ADD |
| Pinned set grows unbounded → eats whole L1 budget | medium | log warning when pinned set exceeds 50% of `l1_budget_tokens`; add `mem unpin --auto-evict` later if it bites |
| `evidence_count` semantics drift across kinds (only skills/lessons get it today) | medium | extend `bump_evidence_count` to also fire on rule/fact/preference references in Phase 2.5 patch — separate commit, test independently |
| `workspace_id` not propagated everywhere mem_save is called | medium | pass through API field once; CLI defaults to `$CODETERM_WORKSPACE_ID` env (already set by GA on every command). Worker / cron paths use `NULL` (global) — explicit |
| User pins a stale item, forgets, L1 carries it forever | low | `mem pinned` lists everything pinned with last-updated; document as user responsibility |
| Score formula gives surprising results — too noisy or too sticky | medium | add `--explain` flag to `mem wake-up` that prints per-item score breakdown for debugging |
| Importance and evidence_count could double-weight each other (importance bumped because frequently cited, then cited bumps again) | low-medium | leave as-is for Phase 2; revisit in Phase 3 if feedback says it produces runaway items |

## Tests

### Unit (`mem/layers.rs`)

- `pinned_item_loads_regardless_of_importance` — pin importance=1 item, assert it appears in L1 over importance=4 unpinned
- `workspace_match_outranks_global_at_margin` — same importance, ws-match wins +0.5
- `pinned_set_over_budget_evicts_lowest_importance_pinned` — log warning emitted
- `evidence_count_log_scaling` — count=10 contributes ~+0.5, count=1000 contributes ~+1.5 (no runaway)

### Integration (Rust)

- `mem_pin_endpoint_round_trip` — POST pin, GET item, assert is_pinned_wakeup=1
- `wake_up_with_workspace_id_env` — set CODETERM_WORKSPACE_ID, call wake-up, assert ws-scoped items prioritized

### Bench (`evals/general-agent/`)

- New eval case: GA in workspace W with pinned `fact_xxx` — assert wake-up output references fact_xxx by content
- A/B: current vs Phase-2 layers.rs on same fixture mem.db; pre-merge κ tracked

## Rollout steps

1. Schema migration commit (`fix(mem): add is_pinned_wakeup + workspace_id`)
2. CRUD path commit (`feat(mem): mem_items.is_pinned_wakeup + workspace_id round-trip`)
3. Layer selection commit (`feat(mem): two-pass L1 selection with pin + ws + evidence_count score`)
4. CLI commit (`feat(cli): mem pin/unpin/pinned + mem save --pin --workspace`)
5. Config commit (`feat(config): memory.l1_budget_tokens`)
6. Extract `bump_evidence_count` to fire on rule/fact references too (Phase 2.5)
7. Tests commit (each commit grew its own test, this consolidates eval-set)
8. Doc bump — `docs/MEMORY.md` + new GA prompt addendum re: pinning UX

Each step is a separate commit on `develop` with self-contained tests so
revert is single-commit.

## Phase 3 outlook (DEFER — research first)

| Item | Status |
|------|--------|
| MemRouter LLM admission classifier | research spike — read arxiv 2605.00356 + Letta core-memory docs; decide go/no-go after Phase 2 ships |
| Lane-based selection (separate quotas per kind) | design after Phase 2 user feedback collected |
| Learned ranking (gradient-boosted features) | needs training data — collect 30 days of (wake-up snapshot, agent recall hit/miss) first |
| Cross-machine pinning sync via mesh | depends on mesh write-replication (separate plan) |

## Open questions answered

- **Q1: cited_count vs evidence_count?** → reuse `evidence_count` (already in
  schema, bumped on success-shaped agent sessions, semantic match)
- **Q2: workspace_id NULL vs scoped fallback?** → workspace_id=NULL means
  global, included for everyone; scoped means included only for matching
  workspace. When GA not in a workspace, only globals load (Pass 1 SQL
  already handles via `IS NULL OR =`)
- **Q3: pin vs importance precedence?** → pin strictly wins. Pinned items
  always load before any unpinned ranking happens
- **Q4: MemRouter classifier needed?** → defer. Lane-based selection alone
  may cover the niche. Decide after Phase 2 in production for 30 days

## References

- `docs/plans/references/mempalace.md` — L0/L1 layered loading pattern (we
  borrowed terminology; their hierarchical "Palace" routing is interesting
  but we don't need it for Phase 2)
- Letta core-memory docs (https://docs.letta.com/guides/ade/core-memory/) —
  similar pin / archival distinction; our `is_pinned_wakeup` matches their
  "core memory block" concept
- arxiv 2605.00356 — MemRouter admission classifier (Phase 3 reference,
  not adopted yet)
- `src-tauri/src/mem/auto_extract.rs:266` — existing `evidence_count` bump
  point ("success-shaped" detection logic, must extend in Phase 2.5)
- `src-tauri/src/mem/layers.rs` — current selection algorithm (the file
  this plan replaces)

<!-- revision: 1 -->
