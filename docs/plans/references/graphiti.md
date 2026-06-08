# Reference: graphiti

**Source:** https://github.com/getzep/graphiti
**License:** Apache-2.0
**Imported:** 2026-05-07
**Local clone:** removed after analysis (re-clone to verify citations)
**Relevant to:** [`docs/plans/mem-optimization.md`](../mem-optimization.md) — Track A (structured extraction), Track C (graph maintenance), schema additions

## What it is

Zep's temporal knowledge graph library, the lib cognee delegates to for its "temporal" feature. Apache-2.0, Python, async-first. Backends: Neo4j (primary), FalkorDB, Kuzu, Neptune. Built around the *episode* model: text + reference time → triples with bi-temporal validity. Strongest prior art for our `valid_from`/`valid_to` design — they have a more complete bi-temporal model and an LLM-driven contradiction detector we don't have.

## Architecture in one paragraph

`add_episode(episode_body, reference_time, ...)` is the ingest verb. Extraction is two LLM passes: first pulls entities + edges from the episode text, second extracts `valid_at`/`invalid_at` timestamps for each edge using a smaller model. Edges have **four temporal columns** — `created_at`, `valid_at`, `invalid_at`, `expired_at` — distinguishing transaction time (when the system saw it) from valid time (when the fact was true in the world). Contradiction detection runs as a single LLM call that sees both new candidate facts and existing potentially-conflicting facts with merged continuous indices, returning indices of contradictions; a subsequent deterministic temporal-overlap pass invalidates older edges when newer `valid_at` supersedes them. Search supports three fusion strategies (RRF, MMR, cross-encoder rerank) plus node-distance and episode-mention reranking. Multi-tenancy via mandatory `group_id` on every node and edge.

## Specific findings

### Bi-temporal model

`graphiti_core/edges.py:L271-L279` — `EntityEdge` carries:

| Column | Meaning |
|---|---|
| `created_at` | Transaction time start — when the system saw this edge |
| `valid_at` | Valid time start — when the fact became true in the world |
| `invalid_at` | Valid time end — when the fact stopped being true in the world |
| `expired_at` | Transaction time end — when the system marked the edge invalid |

`EpisodicEdge` and other edge types only carry `created_at`. Only `EntityEdge` is fully bi-temporal.

This is the canonical bi-temporal model from temporal database research: two independent time axes, four columns. We have `valid_from` / `valid_to` (= `valid_at` / `invalid_at`) and `created_at`. We're missing `expired_at` (system-side invalidation, separate from world-side).

### Episode model

`add_episode()` (`graphiti_core/graphiti.py:L933`) takes:

- `episode_body: str` — the text chunk
- `reference_time: datetime` — the wall-clock time the episode happened
- `name`, `source` (text/json/url), `source_description`
- Optional `saga` (groups episodes into a sequence)

Extraction chain (`graphiti_core/utils/maintenance/edge_operations.py:L116-L321`):

1. LLM call #1 — `prompt_library.extract_edges.edge()` (line 202) — extracts entities + edges from the episode, given `episode_body + nodes + previous_episodes + reference_time + edge_types`. Returns `ExtractedEdges`.
2. LLM call #2 — `_extract_edge_timestamps()` (`L575-L619`) — smaller model (`ModelSize.small`), extracts `valid_at`/`invalid_at` per fact from fact text + reference_time.

For multi-episode batches, each fact is attributed to the episode it came from (L170-180), so per-episode timestamps resolve correctly.

### Contradiction detection

`graphiti_core/utils/maintenance/edge_operations.py:L622-L824` — hybrid LLM + deterministic.

**LLM phase** (`L715-L721`, prompt at `graphiti_core/prompts/dedupe_edges.py:L43-L100`):

- Existing edges + new candidates passed in **a single list with continuous indexing**. The LLM sees all candidates and existing facts together.
- Returns `contradicted_facts: list[int]` — indices of edges contradicted by the new fact.
- Prompt note: "A fact from EXISTING FACTS can be both a duplicate AND contradicted." (`dedupe_edges.py:L81`)

**Deterministic phase** (`resolve_edge_contradictions`, `L537-L572`):

- Compares `valid_at`/`invalid_at` of the contradicted edges.
- If `candidate.valid_at > existing.valid_at` → expire the older edge: `invalid_at = candidate.valid_at, expired_at = utc_now()` (L814-815).
- If `candidate.valid_at < existing.valid_at` → mark the new edge as expired instead.

Pure datemath, no LLM needed for the resolution itself once the LLM has flagged contradictions.

### Time-filtered queries

Surprisingly, **graphiti does not expose built-in time filtering in queries**. `search()` (`graphiti_core/search/search.py:L98`) returns all matching edges; the application layer filters with `valid_at <= query_date AND (invalid_at IS NULL OR invalid_at > query_date)`. Our `mem graph related --as-of <date>` is more agent-friendly — we already do better here.

### Hybrid search

`graphiti_core/search/search_config_recipes.py:L33-L150` — three fusion recipes:

1. **RRF** — BM25 + cosine, standard reciprocal-rank fusion.
2. **MMR** (Maximal Marginal Relevance) — penalizes redundant results, tunable lambda.
3. **Cross-encoder rerank** — BM25 + cosine + graph BFS → cross-encoder rescoring of top-k.

Plus secondary reranks:
- **Node distance** — re-ranks edge results by shortest path to query context.
- **Episode mention** — re-ranks by mention frequency in cited episodes.

### Multi-tenancy

`group_id` is mandatory on every node and edge (`Edge` base class, `EntityEdge`, etc.). Queries scoped via `WHERE e.group_id IN $group_ids`. No cross-group queries.

## Why it's relevant to codeterm mem

**Three takeaways worth lifting:**

1. **Add `expired_at` to `relations` schema.** We track `valid_from` / `valid_to` (valid time only). Adding `expired_at` (transaction time end — when the system marked the relation invalid, separate from when the fact stopped being true) gives us proper bi-temporal storage. Lets us answer "what did the system believe was true on date X" vs "what was actually true on date X" — useful for audit + replay. Cheap migration (add column, default NULL). → Track A (or new Track E: schema upgrade).

2. **Contradiction-detection-with-merged-indices.** When the auto:save LLM extractor sees a new fact, it should also receive a list of existing-related facts with continuous indices and return indices of contradictions. Then a deterministic pass invalidates the older fact's edge with `valid_to = new_fact.valid_from, expired_at = now()`. We currently never invalidate — facts pile up alongside contradictions. → Track A2 (Pydantic-style extractor) gets a contradiction step.

3. **Lightweight timestamp-extraction with a smaller model.** Instead of cramming temporal extraction into the main extraction prompt, do a second small LLM pass that extracts only `valid_at`/`invalid_at` from fact + reference_time. Cheaper, more focused, easier to evaluate independently. → Track A2 sub-step.

**Bonus:** their deterministic temporal-overlap resolution is pure Python datemath; equivalent Rust logic in `mem dream` deep-pass would catch obvious contradictions even if the LLM missed them at extraction time. → Track C (a `temporal_invalidate` step alongside reap and merge).

## What's NOT applicable

- **Multi-graph driver abstraction (Neo4j, FalkorDB, Kuzu, Neptune).** We have SQLite. Cypher-to-SQL translation is unjustified complexity at our scale.
- **Mandatory `group_id` partitioning.** Single-user; we have `scope=global/workspace` as an optional metadata tag, not a join key.
- **Cross-encoder reranking as a first-class search recipe.** We have an opt-in `--rerank` for LLM rerank; cross-encoder is a different model class with extra dependencies. Park.
- **Their lack of agent-friendly time filters at the query layer.** We already have `mem graph related --as-of` and `mem graph timeline`. Don't downgrade.

## Code provenance

graphiti_core/edges.py — bi-temporal columns
graphiti_core/graphiti.py — `add_episode` entry point
graphiti_core/utils/maintenance/edge_operations.py — extraction, contradiction, invalidation
graphiti_core/prompts/dedupe_edges.py — LLM contradiction prompt
graphiti_core/search/search_config_recipes.py — hybrid search recipes
