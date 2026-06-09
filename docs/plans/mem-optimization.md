# mem optimization plan

> **Status (2026-05-07):** drafted, not started. Origin: comparative review of cognee, mem0, Letta (MemGPT), graphiti, and Memary against codeterm mem. Per-library findings live in [`references/`](references/) (one doc per source). The mem store is accumulating low-value auto-extracted fragments and duplicate entities that the current `mem dream` consolidation does not garbage-collect; extraction quality drifts because output is free-form; retrieval has a documented FTS-vs-vector RRF imbalance. This plan addresses extraction, retrieval, schema, and maintenance as one connected set of changes, drawing from prior art where it fits and explicitly rejecting where it doesn't.

The persistent mem store on this machine has grown to **132 items / 154 entities / 327 relations / 285 vectors**. Shape of the problem:

- Entities exceed items (1.17×). Healthy graphs have far fewer entities than items because entities aggregate across items. Drift caused by free-form extractor output (`Tima` / `tima` / `Тима` / `Tima Vasilenko` all distinct nodes).
- ~10+ items match the shape `auto: User has mentioned (instead_of|rather_than|use_instead_of): <fragment>` — short context-stripped sentence pieces with `tags=[auto-extracted]`. Body example: `"User has mentioned (instead_of): actual results"`. Useless for recall; embed as vectors anyway; count toward graph density.
- Recent dream reports ("No new duplicates identified") miss this — body-level dedup compares whole bodies, not shape patterns; entities have no bodies at all.
- 24-query doc bench (`note_doc_bench_dev_2026_04_16_results`): 70% hit@1 / 83% hit@3 — workable; documented FTS-vs-vector RRF imbalance causes specific misranks (`learning_fts_keyword_precision_fragile_in_rrf_hybrid_search`).

`mem dream` deep-pass already classifies, links, and audits. It does not delete, does not merge, does not invalidate stale facts, and does not address retrieval quality. This plan adds those, kills the upstream junk source, and incidentally improves search ranking on shape-obvious queries.

## References

The plan synthesizes ideas from five existing memory libraries. Each has its own analysis doc — citations and code provenance live there:

- [cognee](references/cognee.md) — two-tier memory + improve() bridge, rule-based query router, LLM structured extraction with orphan-edge filtering.
- [mem0](references/mem0.md) — `linked_memory_ids` in extraction output, entity-name vectors as secondary index, named-weight scoring fusion.
- [Letta / MemGPT](references/letta.md) — agent-curated core memory blocks, debug-annotated search results (`rrf_score`/`vector_rank`/`fts_rank`), git-backed memory filesystem.
- [graphiti](references/graphiti.md) — four-column bi-temporal model (adds `expired_at`), LLM contradiction detection with merged indices, lightweight timestamp extraction with smaller model, deterministic temporal-overlap resolution.
- [Memary](references/memary.md) — query-time LLM synonym expansion (only real takeaway; codebase otherwise prototype-grade).

The cognee reference summarizes the full report at `~/.codeterm/general/research-cognee-vs-codeterm-mem.md` (mem note `note_research_cognee_vs_codeterm_mem`).

## Current shape

**Extraction pipeline on save** — two parallel mechanisms write to the graph:

1. **LLM auto:save worker** — Claude Haiku, intent `extract:<item_id>`, runs in a hidden pane, emits *free-form* `kind=fact` items + entities + relations. Output quality is good when it succeeds (e.g. `fact_decision_mem_save_must_never_block_on_async_llm_steps` was correctly extracted from a research note). Failure mode visible in worker logs: `idle for 30s — claude exited without result event` (model wrote text we couldn't parse, silently dropped). Also hits Anthropic rate limits.
2. **Regex pattern extractor** — 16 hardcoded patterns (`I prefer X`, `instead of Y`, `rather than Z`, `use instead of …`) running over `kind=conversation`/`note` bodies. Captures the substring after the trigger and saves it standalone with `body = "User has mentioned (<rel>): <captured-fragment>"`. **This is the source of the shape junk.**

**Dream consolidation** (`codeterm mem dream`, Rust binary, called nightly via cron `0 3 * * *` with ±90 min jitter):

- Light pass: classification, area assignment, dedup *check* (does not delete).
- Deep pass: graph maintenance — adds missing `instance_of`/`part_of`/`addressed_by` relations; audits density (~0.13 edges/entity baseline); writes a markdown report to `daily_dream_<date>`.
- **No deletion phase. No merge phase. No contradiction-resolution phase. No retrieval-quality investigation.**

**Retrieval pipeline:**

- `mem search` runs FTS5 (BM25) + fastembed-bge-small-en (384d) + RRF fusion in one call. Optional `--rerank` does an LLM rerank on the top-K. Cosine dedup at write time prevents near-duplicate vectors on the same body.
- Documented imbalance: FTS5 returns fewer hits than the vector retriever on multi-token queries, so vector candidates dominate the fused list. Specific misrank example: `"PgBouncer transaction mode autovacuum"` returns `windows_display` top-1 instead of `postgresql_tuning`.

**Existing primitives we can reuse:**

- `mem graph alias --from <loser> --canonical <winner>` — redirects all `relations` from loser to winner and deletes the loser. Atomic.
- `mem update <id> --body "..."`, `mem delete <id>`, `mem worker spawn --intent ... --task ...`, `mem extract-pending` — building blocks.
- Vectors are computed for every item — clustering by cosine has no extra cost beyond a scan.
- `--scope`, `--area`, `--kind` filters on `mem search` already exist.
- Graph relations already carry `valid_from`/`valid_to` columns plus `confidence`.

---

## Track A — extraction quality

The upstream problem. Good extraction → less cleanup work downstream. Three phases.

### A1. Kill the regex pattern extractor — small, 1 session

Disable the 16-pattern regex extractor that produces `User has mentioned (...)` fragments. The LLM auto:save worker covers the same use case better with full context.

- Locate the patterns (Rust side; entry point near `mem extract-pending`).
- Decision: delete entirely, OR gate behind a config flag (`mem.regex_extractor.enabled = false` default).
- Migration: existing junk items remain until C1 reaps them.

**Outcome:** zero new `User has mentioned ...` items.

### A2. Structured extraction via Anthropic tool-use — medium, 1-2 sessions

Make the LLM auto:save worker emit a typed graph fragment via Anthropic tool-use instead of free-form text. Mirrors cognee's `extract_graph_from_data.py:L181` pattern (Pydantic-style schema with orphan-edge filtering); enriched with mem0's `linked_memory_ids` and graphiti's contradiction detection.

**Schema** (Rust struct + JSON Schema via `schemars`):

```rust
struct ExtractionOutput {
    entities: Vec<Entity>,         // { name, entity_type, summary? }
    relations: Vec<Relation>,      // { from, rel, to, valid_from?, valid_to?, confidence }
    facts: Vec<Fact>,              // { title, body, kind, area, importance, linked_to?: Vec<MemId> }
    contradictions: Vec<usize>,    // indices into the existing-facts context (see below)
    extras: Option<Vec<String>>,   // catch-all for things outside the schema
}
```

Closed enums:
- `entity_type`: `Person | Project | Tech | Machine | Event` (matches existing taxonomy).
- `kind`: existing mem_item kind enum.

**Pipeline:**

1. **Build extraction context.** Query top-K existing facts/entities semantically related to the new item (cosine over body vectors, top 10 facts + relevant entities). Pass them with **continuous indices** alongside the new item, per [graphiti](references/graphiti.md)'s contradiction-detection pattern.
2. **Call Anthropic Messages API** with the tool definition. Force tool-use (model must call `save_extraction`, can't return free text).
3. **Validate post-extraction:**
   - Drop relations whose `from`/`to` aren't in the same response's `entities` list (cognee's pattern).
   - Reject empty entity names; reject malformed dates in `valid_from`/`valid_to`.
   - Resolve `linked_to` references against the context items by index.
4. **Apply contradictions deterministically.** For each contradicted index, look up the existing relation/fact and:
   - If new fact has `valid_from > existing.valid_from` → set `existing.valid_to = new.valid_from`, `existing.expired_at = now()` (graphiti's `resolve_edge_contradictions`).
   - If new fact has `valid_from < existing.valid_from` → mark new as already-expired.
   - Otherwise leave both, log to dream report for human review.
5. **Persist** entities, relations, facts. Tag each with `schema_version=1` so future schema changes can migrate or quarantine old extractions.

**Why each piece matters:**

| Piece | Source | Solves |
|---|---|---|
| Tool-use schema | cognee Pydantic + Anthropic tool-use | Eliminates "claude exited without result event" silent fail |
| Closed `entity_type` enum | (ours, reinforced by mem0/cognee) | Stops `Tima/tima/Tima_K` sprawl |
| Orphan-edge filter | cognee `extract_graph_from_data.py:L181` | Drops invalid relations before DB |
| `linked_to` references | mem0 `linked_memory_ids` | LLM groups related facts at extraction time, less work for C3 |
| Contradiction-with-merged-indices | graphiti `dedupe_edges.py:L43-L100` | Invalidates stale facts when superseded |
| Deterministic temporal resolution | graphiti `resolve_edge_contradictions:L552-L572` | Pure datemath, no LLM needed for invalidation |
| `schema_version` tag | (ours) | Future-proofs migration |

**Cost:** tool-use adds system-prompt + tool-definition overhead per call. Negligible on Haiku.
**Effort:** ~1 day. Worker already calls Anthropic API; change is local — schema definition + tool registration + response parsing + validator + contradiction resolver.

### A3. Lightweight timestamp extraction with smaller model — small, 0.5 session, optional

Borrowed from [graphiti](references/graphiti.md) `_extract_edge_timestamps:L575-L619`. Instead of cramming temporal extraction into the main A2 prompt, add an optional second LLM pass that extracts only `valid_at`/`invalid_at` from a fact + reference_time. Cheaper, more focused, easier to evaluate independently.

- Fires only when A2's main extractor returns a fact with prose temporal language but no parsed dates.
- Uses the same Haiku model (we don't have a "smaller-than-Haiku" tier).
- Skip when fact body has no temporal content.

**Why optional:** A2's main schema accepts `valid_from`/`valid_to` directly. A3 is a safety net for facts where the LLM noticed temporality in prose but didn't fill the structured fields.

---

## Track B — retrieval quality

Investigate and fix the documented FTS-vs-vector RRF imbalance. Independent of A and C.

### B1. Investigate RRF fusion weights — small, 1 session

Hypothesis: standard RRF `1/(k + rank_i)` treats both retrievers symmetrically, but FTS5 returns far fewer candidates than the vector retriever on multi-token queries. The few FTS hits land at very low absolute ranks but their contribution is overwhelmed by 50+ vector candidates each contributing a small score.

Steps:

1. Instrument the existing fuser to log per-query `fts_count`, `vec_count`, and the per-candidate score breakdown for the 24 doc-bench queries.
2. Add **debug-annotated results** as in [Letta](references/letta.md) `agent_manager.py:L2621-L2625` — every hit carries `rrf_score`, `vector_rank`, `fts_rank`. Available via `mem search --debug` or always in `--json` output. Helps diagnose without a separate instrumentation harness.
3. Try alternate fusers:
   - **FTS-count rebalance:** `fts_score *= max(1, vec_count / fts_count)`.
   - **Score-based fusion** (mem0 [`utils/scoring.py`](references/mem0.md)) — normalize raw scores per retriever, fuse with named weights (`semantic_weight`, `bm25_weight`).
   - **Cascade:** if FTS returns ≥N hits and any has very high BM25 score, return FTS-only top-K; else fall back to RRF.
4. Decision criterion: pick the fuser maximizing hit@3 on the doc bench without dropping hit@1.

Ship behind a config flag `mem.search.fuser = rrf | weighted | cascade`.

### B2. Rule-based query router — medium, 1 session, optional

Borrowed from [cognee](references/cognee.md) `query_router.py`. Apply *internally* — the user still types `mem search "..."` with no new flags.

Routing patterns:

| Trigger | Route |
|---|---|
| Quoted exact phrase `"foo bar"` | FTS-only |
| Cypher-shaped (`^MATCH `, `^RETURN `) | FTS-only |
| Year tokens `\b\d{4}s?\b`, `between YYYY and YYYY`, `since\|before\|after\|during` | Temporal traversal first, fall back to vector |
| Code shape (`def `, `import `, `\.py\b`) | FTS-only |
| Default | Full RRF fusion (or whatever B1 picked) |

**Negation suppression** (cognee's pattern): if `not\|n't\|no\|never\|without` appears within 20 chars before a trigger, ignore it.

**Override tracking:** if a user passes a contradicting flag (e.g., `--kind` filter that the router didn't expect), increment a counter for misrouting analysis. No eval set needed; user behavior surfaces patterns.

### B3. Query-time synonym expansion — stretch, optional

Borrowed from [Memary](references/memary.md) `synonym_expand/synonym.py`. One LLM call (Haiku) expands the query before search runs, e.g. `"PgBouncer transaction mode autovacuum"` → `["PgBouncer", "connection pooling", "transaction mode", "autovacuum", "postgres tuning"]`. Each expanded term contributes to the FTS query.

Why stretch: every search becomes one LLM call slower, and at our scale recall already feels fine ("вроде и так нормально"). Worth measuring against the doc bench but only after B1+B2 land — those are zero-LLM-cost fixes.

---

## Track C — graph maintenance in dream

Once extraction (A) and retrieval (B) are addressed, dream cleans what's already in the store. Four phases.

### C1. Reap junk in dream deep-pass — small, 1 session

Add a `reap_junk` step to `mem dream` deep-pass. Removes auto-extracted fragments by shape, not similarity.

Deletion criteria (all must hold):

- `tags` contains `auto-extracted`, AND
- `kind` in `{fact, preference, learning}`, AND
- `len(body) < 80` characters, AND
- `body` matches a junk-shape regex set: `^User has mentioned`, `^auto: `, body is just `<word>: <fragment>`.

Side effects:

- Walk `relations` and remove rows where either endpoint is the deleted item.
- Walk vectors and drop the item's chunks.
- Increment `reaped_count` in the dream report. Per-id list at debug log level.

**Safety guard:** dry-run mode (`mem dream --reap-dry-run`) prints what would be deleted, doesn't write. Promote to default-on after one or two dry runs confirm criteria are conservative.

### C2. Merge near-duplicate entities — small, 1 session

Find and merge duplicate entities. Uses the existing `mem graph alias` primitive.

**Auto-merge** (no human review):

- **Exact-after-normalize** (case-fold, strip punctuation, collapse whitespace) → keep entity with higher `relations` count, alias the other.
- **Substring containment** (`"Tima"` ⊂ `"Tima Vasilenko"`) → keep the longer (more specific), alias the shorter.

**Flag-for-review** (write to dream report under "Merge candidates"):

- **Levenshtein ratio > 0.85** between two normalized names → list pair.
- **Cosine similarity > 0.80** on the canonical name's text-embedding → list pair.

**Entity-name vectors:** borrowed from [mem0](references/mem0.md) — embed each entity name into a secondary index (`{collection}_entities` in mem0; we'd add an `entity_vectors` table). Cost: ~150 entities × 1.5KB ≈ 230KB. Resolves Open Question 3 from the original draft.

Winner selection on ties: higher `recurrence_count`, then earliest `created_at`.

### C3. Merge near-duplicate items — medium, 1-2 sessions

Use existing item vectors to cluster near-duplicate items by cosine; merge with append-not-replace semantics.

Algorithm:

1. For each `kind` × `area` pair, scan items, compute pairwise cosine.
2. **Cosine ≥ 0.95** → auto-merge:
   - Pick winner: `access_count + 2*recurrence_count desc, importance desc, created_at asc`.
   - `winner.body = winner.body + "\n\n---\n\n" + loser.body` (preserve nuances).
   - `winner.tags = sorted(set(winner.tags + loser.tags))`.
   - Redirect `relations` from loser to winner.
   - `mem delete <loser>`. Re-embed winner.
3. **Cosine 0.85–0.95** → emit pair into dream report under "Merge candidates for review."
4. **Cosine < 0.85** → ignore.

**Safety guards:**

- Cap auto-merges per dream run at 10 (one bad threshold can't trash the store).
- Skip if items have different `source` paths (don't cross-merge Librarian-tracked files).
- Skip if items have non-empty graph relations to *different* entities (signal they capture distinct angles).
- A2's `linked_to` field reduces the work here — facts the LLM already grouped at extraction don't need cosine clustering after the fact.

### C4. Temporal-overlap invalidation — small, 1 session

Borrowed from [graphiti](references/graphiti.md) `resolve_edge_contradictions:L552-L572`. Pure Rust datemath; no LLM. Catches contradictions A2 missed at extraction time.

For each `(subject, predicate)` pair with multiple relations:

- Sort by `valid_from`.
- Walk pairs: if relation A has `valid_to = NULL` and B has `valid_from > A.valid_from` AND B is on the same predicate with a contradicting object, set `A.valid_to = B.valid_from, A.expired_at = now()`.
- Idempotent — running twice has no effect.

Logs `invalidated_count` to the dream report.

---

## Track D — schema upgrades

Two changes large enough to call out separately.

### D1. Add `expired_at` column to `relations` — small, requires migration

Borrowed from [graphiti](references/graphiti.md) `edges.py:L271-L279`. We track `valid_from`/`valid_to` (valid time only) and `created_at` (transaction time start). Adding `expired_at` (transaction time end — when the system marked the relation invalid) gives proper bi-temporal storage. Lets us answer:

- "What is true now?" — `expired_at IS NULL AND valid_to IS NULL`.
- "What was the system's belief on date X?" — `created_at <= X AND (expired_at IS NULL OR expired_at > X)`.
- "What was actually true on date X?" — `valid_from <= X AND (valid_to IS NULL OR valid_to > X)`.

Migration: add column, default NULL. C4 and A2 will populate it as relations get superseded.

### D2. Two-tier memory: hot session cache + cold permanent graph — stretch

Borrowed from [cognee](references/cognee.md) `recall.py:L156-L211` and `improve.py:L36`.

Add a separate session tier with its own surface:

- `mem session add/get/search` — keyed by `session_id`, Jaccard-only search (no embedding cost), holds short-lived QA / trace / feedback entries.
- `mem dream`'s deep-pass gains a "promote sessions to graph" step (cognee's stages 1+2: apply feedback weights to graph edges, persist QA).
- Stages 3+4 (memify enrichment, sync graph back to session cache) skipped — solving problems we don't have at our scale.

Why stretch: at 132-item scale, single-tenant, this is overkill. Becomes valuable once we want agents to learn from their own conversations and weight retrieval by past feedback signal. Revisit when an actual session-context retrieval pain point surfaces.

### D3. Letta-style agent-curated core memory blocks — stretch, future

Borrowed from [Letta](references/letta.md). A small set of always-loaded, agent-editable blocks (`current-task`, `working-set`, `persona`) maintained via `mem core append/replace/patch` tools. Different from D2 (which is automatic LLM bridging) — these are explicit, agent-driven. Maps to existing skill-folder workflow if extended.

Why stretch: changes the agent's interaction model from "save items, search later" to "actively curate context blocks." Worth considering once we have observed agent patterns that demand it. Park.

---

## Execution order

1. **A1** — kill the regex extractor. Smallest unit; biggest immediate quality win; prerequisite for C1 to be a finite cleanup.
2. **C1** — reap existing junk. Once A1 lands, C1 has a finite set to clean.
3. **D1** — add `expired_at` column. Unblocks A2 contradiction handling and C4. Cheap migration.
4. **A2** — structured tool-use extraction with contradictions. Reduces rate at which C2/C3 must clean. Independent of B; can develop in parallel.
5. **C4** — temporal-overlap invalidation. Cheap deterministic Rust on top of D1.
6. **C2** — entity merge (with mem0-style entity-name vectors). Cheap because `mem graph alias` exists.
7. **B1** — RRF imbalance investigation + Letta-style debug annotations. Independent track; no dependency on A/C/D.
8. **C3** — item merge. Biggest C-track effort. Defer if corpus stabilizes after A1+C1+C2.
9. **A3** — lightweight timestamp extraction. Optional safety net for A2.
10. **B2** — query router. Optional incremental win after B1.
11. **D2/D3/B3** — stretch tracks. Only as user-pain-driven follow-ups.

## Non-goals

- **No re-architecting `mem dream` into an LLM-driven agent.** Rust task today; deletion / merge / fusion / invalidation logic stays in Rust — faster, deterministic, testable.
- **No multi-tenant scoping changes in `mem search`.** Single-user, single-tenant. Existing `--scope`/`--area`/`--kind` filters cover listing-style cases. Cognee's per-dataset gating is access control wearing a search-feature costume; we don't need it.
- **No swap of fastembed for a heavier embedding model now.** Doc-bench numbers don't justify; suspected root cause is fusion weights (B1).
- **No Cypher pass-through, no Neo4j temporal stack.** Already covered by our SQLite graph and `mem graph relate --valid-from --valid-to`. Cognee delegates temporal to graphiti_core hardcoded to Neo4j; we'd be downgrading.
- **No 15-mode SearchType enum exposed to callers.** Internal routing fine (B2); public surface stays single-call.
- **No mandatory `user_id` / `agent_id` / `run_id` scoping (mem0 style).** Single-user; metadata-as-filter is enough for the listing cases.
- **No long-lived agent server (Letta style).** CLI-driven, stateless per invocation. Memory mutations are load → mutate → save.

## Open questions

1. **Where exactly does the regex extractor live in code?** Invoked via `mem extract-pending` and from the auto:save worker spawn path. Need to find the Rust file and confirm whether patterns are hardcoded in source or table-driven (A1 decision tree depends on this).
2. **Reap criteria conservatism (C1).** Is `len(body) < 80 AND tags has auto-extracted` strong enough alone, or is the explicit shape regex set required? A scan of current items can answer.
3. **A2 fallback path.** If a tool-use call fails (rate limit, model unavailable), do we retry with the same prompt later (`mem extract-pending` already requeues) or quietly fall back to free-form? Probably retry-only — silent free-form fallback regrows the entity-type sprawl A2 prevents.
4. **Entity-name vector model (C2).** Reuse fastembed-bge-small-en, or a smaller/cheaper model since entity names are short tokens? Worth measuring on a sample.
5. **C3 winner-selection composite.** Is `access_count + 2*recurrence_count` the right weighting, or should `recurrence_count` dominate (re-saving signals confirmed value, while access_count signals discoverability)?
6. **Should the merge-candidates list (C2 fuzzy + C3 mid-confidence) become a structured `mem_item` so the user can act from the Memory Panel UI**, instead of being buried in the daily dream markdown? Probably yes; adds UI work.
7. **Tool-use schema versioning (A2).** When the schema changes, do we re-extract historical items, quarantine old extractions, or just tag them and let dream eventually upgrade? Probably tag-and-leave; document the migration when first schema change happens.
8. **B3 cost ceiling.** If query-time synonym expansion lands, do we cache expansions by query hash to amortize the LLM call? Probably yes (LRU cache, 1-day TTL).

## Where this came from

Conversation on 2026-05-07 reviewing five existing memory libraries against codeterm mem. Cognee does aggressive entity extraction via LLM but skips dedup beyond content hash; mem0 has clean hybrid scoring + entity-name vectors; Letta has agent-editable core memory + debug-annotated retrieval; graphiti has the most complete bi-temporal model and LLM contradiction detection; Memary is mostly a research prototype but has the synonym-expansion idea. Our store has the opposite shape from cognee — limited extraction quality and no GC at all, but stronger native bi-temporal facts and a one-call hybrid retriever. The tracks here close the gaps without inheriting the downsides (per-dataset auth, OpenAI-default dependency, Neo4j-bound temporal stack, long-lived server architecture).
