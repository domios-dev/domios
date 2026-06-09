# Reference: Memary

**Source:** https://github.com/kingjulio8238/Memary
**License:** MIT
**Imported:** 2026-05-07
**Local clone:** removed after analysis (re-clone to verify)
**Relevant to:** [`docs/plans/mem-optimization.md`](../mem-optimization.md) — limited; Track B (retrieval) and observations only

## What it is

A research-grade autonomous-agent memory system, ~1k LoC of Python on top of llama-index 0.11 + Neo4j (or FalkorDB) + LangChain 0.1. Single-contributor project (Gal Shubeli, memary Labs), v0.1.2, single-commit git history (PR #61, May 2026). Not production code.

## Architecture in one paragraph

Three decoupled stores: a **MemoryStream** (timestamped `{entity, datetime}` JSON file), an **EntityKnowledgeStore** (frequency table compressing the stream — `{entity, count, last_seen}`), and a **Knowledge Graph** built separately by `KnowledgeGraphIndex.from_documents(... max_triplets_per_chunk=8)` from llama-index, stored in Neo4j or FalkorDB. The stream/EKS track entity salience over time; the KG holds semantic relationships. They never unify into a single store. Recall expands the query through an LLM-powered synonym function, then runs `retriever_mode="keyword"` over the KG. A ReAct agent wraps the whole thing with native tool-use.

## Specific findings

### Three-tier separation (entity salience vs semantic graph)

`memory_stream.py` — JSON file of `{entity, datetime}` events, append-only.
`entity_knowledge_store.py` — JSON file of `{entity, count, last_seen}`, derived from the stream.
KG (Neo4j/FalkorDB) — built from documents via `SimpleDirectoryReader` + `KnowledgeGraphIndex.from_documents`.

The defining choice: **entity salience and semantic relationships live in different stores.** mem0 collapses both into a vector index; Letta uses simpler in-memory dicts; cognee unifies into one graph. Memary keeps them deliberately separate.

### Synonym expansion at query time

`synonym_expand/synonym.py:L9-L42` — `custom_synonym_expand_fn`: an LLM call expands query terms into synonyms before retrieval. Example: `"Palantir"` → `["Palantir", "Palantir technologies", "Palantir technologies inc"]`.

Plugged into the retriever (`base_agent.py:L109-L119`):

```python
graph_rag_retriever = KnowledgeGraphRAGRetriever(
    storage_context=self.storage_context,
    retriever_mode="keyword",
    synonym_expand_fn=custom_synonym_expand_fn,
)
```

Useful pattern: an extra LLM pre-step that expands the query, instead of trying to make embeddings handle terminology variants on their own.

### Backend dual-mode

Neo4j or FalkorDB swappable via env var (`base_agent.py:L84-L102`). FalkorDB has first-class support for multi-user isolation via `select_graph(user_id)`.

### No bi-temporal versioning

Linear timestamps only. No `valid_from`/`valid_to`. Inferior to graphiti, cognee, and our own design.

### Code maturity

- **Single commit** in git history. Either repo was published as one snapshot dump from a private branch, or the project has very few changes.
- **Single contributor.**
- **Pinned to llama-index 0.11.4 and LangChain 0.1.12** — both already deprecated.
- **Tests** only in `dev/KG_memory_stream/tests/`, not packaged.
- **No CI, no release tags.**

This is a research prototype, not a production library.

## Why it's relevant to codeterm mem

**One real takeaway, one possible:**

1. **Query-time LLM synonym expansion.** A small extra LLM call (Haiku-fast) expands the query before the FTS+vector retrievers run. Could materially help our documented imbalance — `"PgBouncer transaction mode autovacuum"` would expand to include `"connection pooling"`, `"postgres tuning"`, etc., giving FTS more anchor tokens. → Possible Track B addition (query expansion as a pre-search step), but expensive (LLM call per query). Worth measuring against the doc bench before committing.

2. **The three-tier observation:** entity salience is genuinely a different signal from semantic relationships. We currently mash them together (entity creation timestamp + relation count). Worth thinking about whether an "access frequency" view of entities would help dream's importance scoring. → Speculative, parking lot.

## What's NOT applicable

- **Decoupled stream / EKS / KG architecture.** Adds three sources of truth and three sync problems. Our unified bi-temporal SQLite is simpler and richer. The conceptual separation is interesting; the implementation isn't.
- **llama-index dependency stack.** Heavy Python dependency tree, deprecated versions, opinionated retrieval patterns. None of this ports to Rust + SQLite.
- **`retriever_mode="keyword"` only KG search.** No vector + keyword hybrid; no reranking. Our existing FTS+vector+RRF is materially better.
- **Production-grade claims.** Treat citations from this repo as exploratory inspiration only.

## Code provenance

src/memary/agent/base_agent.py — main orchestrator
src/memary/memory/memory_stream.py — event log
src/memary/memory/entity_knowledge_store.py — frequency table
src/memary/synonym_expand/synonym.py — LLM query-expansion hook
