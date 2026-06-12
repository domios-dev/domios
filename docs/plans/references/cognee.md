# Reference: cognee

**Source:** https://github.com/topoteretes/cognee
**License:** Apache-2.0
**Imported:** 2026-05-07
**Local clone:** removed after analysis (re-clone to verify)
**Full report:** `~/.codeterm/general/research-cognee-vs-codeterm-mem.md`
**Mem note:** `note_research_cognee_vs_codeterm_mem`
**Relevant to:** [`docs/plans/mem-optimization.md`](../mem-optimization.md) — Tracks A, B, C, D

## What it is

The headline open-source memory library positioning itself as "the brain behind your agents." ~17k stars, hotfix-active (1.0.6 → 1.0.8 in one day around 2026-05-06), open-source Apache-2.0 + paid SaaS at cognee.ai. Python, async, OpenAI-default. Has a first-party Claude Code plugin (`topoteretes/cognee-integrations`) — direct competitor for the niche we're in.

## Architecture in one paragraph

Top-level verbs `remember()` / `recall()` / `forget()` / `improve()` (`cognee/api/v1/{remember,recall,forget,improve}/`). Two storage tiers: a fast **session cache** (fscache/Redis/tapes adapters, keyed `(session_id, user_id)`, Jaccard token-overlap search) and a **permanent knowledge graph** (Ladybug/Kuzu graph + LanceDB/ChromaDB/PGVector vector + SQLite metadata). `remember(data, session_id=...)` writes only to the session cache for low latency; `improve()` runs as a 4-stage background bridge that promotes session content into the graph, applies feedback weights from session retrieval traces to graph edges, persists agent trace steps, runs memify enrichment (triplet embeddings), and syncs newly-enriched edges back into the session cache as JSON-lines. Search is router-driven: `recall()` runs `query_router.route_query()` — 16 weighted regex rules with negation-window suppression — to pick one of 15 SearchType modes (`SUMMARIES`, `CHUNKS`, `CHUNKS_LEXICAL`/Jaccard, `RAG_COMPLETION`, `GRAPH_COMPLETION`, `GRAPH_COMPLETION_COT`, `CYPHER`, `TEMPORAL` etc.) without an LLM call. Temporal awareness delegates to graphiti_core hardcoded to Neo4j (so unusable on default Ladybug). Defaults: `gpt-5-mini` extractor, `text-embedding-3-large` 3072d embedder, both OpenAI — `cognify()` errors without `LLM_API_KEY`.

## Specific findings

### Two-tier memory + improve() bridge

- **Session cache** (`cognee/infrastructure/databases/cache/`): pluggable fscache/Redis/tapes; keyed by `(session_id, user_id)`; holds `QAEntry`, `TraceEntry`, `FeedbackEntry`. Search is Jaccard token overlap (`cognee/api/v1/recall/recall.py:L156-L211`). Fast, ephemeral.
- **Permanent graph** (vector + graph backends). Heavy: full `cognify()` pipeline.
- **`improve()` bridge** (`cognee/api/v1/improve/improve.py:L36`) — 4 stages:
  1. Apply feedback weights to graph edges from session retrieval traces (`feedback_alpha=0.1`).
  2. Persist session Q&A as `node_set="user_sessions_from_cache"`.
  2b. Persist agent trace steps as `node_set="agent_trace_feedbacks"`.
  3. Memify enrichment (triplet embeddings).
  4. Sync recent graph edges back into session cache as JSON-lines.
  Per-session lock prevents concurrent runs (`cognee/api/v1/improve/improve.py:L122`).

### Rule-based query router

`cognee/api/v1/recall/query_router.py` — 232 lines, no LLM call:

- 16 weighted regex rules (Cypher-shape → `CYPHER`, year token `\b\d{4}s?\b` → `TEMPORAL`, "summarize/tldr" → `GRAPH_SUMMARY_COMPLETION`, code keywords → `CODING_RULES`, etc.).
- Negation-window suppression: a match is dropped if `not|n't|no|never|without|lack` appears within 20 chars before the match start.
- Confidence: `is_confident = best_score >= 2 * runner_up_score`.
- `record_override(routed, override)` tracks misroutings whenever the user passes a `query_type` that disagrees with the route — surfaces patterns without needing an eval set.

### LLM-driven structured extraction

`cognee/tasks/graph/extract_graph_from_data.py:L128`:

1. Each chunk → `extract_content_graph(text, graph_model, custom_prompt)`.
2. Default `graph_model = KnowledgeGraph` (Pydantic with `nodes: List[Node]`, `edges: List[Edge]`).
3. Drop edges whose `source_node_id`/`target_node_id` aren't in the same response's `nodes` list (`L181-L188`).
4. Ontology resolver validates entities against an optional ontology (or default permissive); validated nodes expand into graph DataPoints.
5. Cascade extractor (`cognee/tasks/graph/cascade_extract/`) does multi-stage: extract content-nodes → extract relationship names → extract edge triplets. Trades latency for precision.

### 15 search modes + router pre-pick

`cognee/modules/search/types/SearchType.py:L5` — `SUMMARIES, CHUNKS, RAG_COMPLETION, TRIPLET_COMPLETION, GRAPH_COMPLETION, GRAPH_COMPLETION_DECOMPOSITION, GRAPH_SUMMARY_COMPLETION, GRAPH_COMPLETION_COT, GRAPH_COMPLETION_CONTEXT_EXTENSION, CYPHER, NATURAL_LANGUAGE, TEMPORAL, CODING_RULES, CHUNKS_LEXICAL, FEELING_LUCKY`.

`CHUNKS_LEXICAL` is `JaccardChunksRetriever` (set/multiset Jaccard over lowercase word tokens). `CHUNKS` is `ChunksRetriever` (vector). **Two separate retrievers**, no fused-rank retriever — the router picks one, never both.

### Temporal stack (delegated)

`cognee/tasks/temporal_awareness/build_graph_with_temporal_awareness.py:L9` — literally `from graphiti_core import Graphiti` and `Graphiti(url, "neo4j", password)`. Hardcoded Neo4j requirement. `GraphitiNode` (`graphiti_model.py`) has only `content`/`name`/`summary` — no validity columns of its own. So users on the default Ladybug backend can't use temporal at all.

## Why it's relevant to codeterm mem

Five major borrows, three explicit don'ts. Detailed in the main plan; condensed here:

**Borrow:**

1. **Two-tier session-cache + improve() bridge** with feedback weights mutating graph edges. → Track D2 (stretch).
2. **Rule-based query router** as an internal optimization for `mem search`. → Track B2.
3. **LLM-structured extraction with edge-validation drop-orphans** (`extract_graph_from_data.py:L181`). → Track A2.
4. **`improve()`-style multi-stage maintenance pipeline** as a model for our `mem dream` deep-pass structure. → Track C structure.
5. **Cognify task abstraction with `run_in_background` flag everywhere** — never block the foreground call on heavy LLM work. → Track A2 invariant.

**Don't:**

1. **Default OpenAI dependency.** Their `cognify()` errors without `LLM_API_KEY`. We are local-first by design.
2. **Graphiti / Neo4j temporal stack.** Hardcoded backend, separate process. Our SQLite bi-temporal is more accessible.
3. **15-mode `SearchType` enum exposed to callers.** Internal routing fine; public surface stays single-call.

## Code provenance

cognee/api/v1/remember/remember.py — top-level remember verb
cognee/api/v1/recall/recall.py — top-level recall verb + session search
cognee/api/v1/recall/query_router.py — 16 rule router
cognee/api/v1/improve/improve.py — 4-stage bridge
cognee/api/v1/cognify/cognify.py — 6-stage pipeline
cognee/tasks/graph/extract_graph_from_data.py — LLM structured extraction
cognee/tasks/temporal_awareness/build_graph_with_temporal_awareness.py — graphiti delegation
cognee/modules/search/types/SearchType.py — 15-mode enum
cognee/modules/retrieval/jaccard_retrival.py — lexical retriever
cognee/infrastructure/databases/cache/ — session cache adapters
