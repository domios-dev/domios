# Memory & Knowledge Architecture Plan

**Status:** Research complete, implementation not started
**Date:** 2026-04-06
**Parent:** `docs/plans/plugin-architecture.md` Phase 3 (Memory Plugin) — this is the detailed design.

## Problem

Agents (Claude, Codex, any) running in CodeTerm tabs have no persistent memory. Each session starts blank. Skills (44+ in the user's library) have no single source of truth — Claude reads from `~/.claude/skills/`, Codex has no equivalent, and every agent would need copies.

Key questions this plan answers:
1. Where do memories live and how are they searched?
2. How do skills get delivered to any agent without duplication?
3. How does the knowledge graph integrate?
4. How does compaction/consolidation work?
5. What's core vs plugin?

## Research Sources

| Source | Key takeaway |
|--------|-------------|
| OpenClaw `context-engine/` | Pre-compaction flush: silent turn saves context before summarization destroys it |
| OpenClaw `memory-core/` | SQLite + sqlite-vec for hybrid search (vector + BM25 + MMR + temporal decay) |
| OpenClaw dreaming phases | Light/Deep/REM consolidation on cron schedule, modeled after sleep |
| Claude Code internals | Skills: descriptions at start (~450 tokens), full content on-demand. Rules: `~/.claude/rules/*.md`. Memory: first 200 lines of MEMORY.md |
| User's `ontology` skill | Typed graph model, 30+ entity types, schema validation, relation traversal |
| User's `self-improving-agent` | Capture pipeline: activator hook → error-detector → .learnings/ → promote to MEMORY.md/SOUL.md |
| CodeTerm cron system | Already supports agent spawn + completion tracking + log archival |
| SQLite docs (`JSON1` / `JSONB`, `FTS5`) | One local DB file can hold structured entities, relations, full-text search, and optional opaque JSON payloads |

## Architecture Overview

```
┌──────────────────────────────────────────────────────────────┐
│                        CORE (src-tauri)                       │
│                                                               │
│  src-tauri/src/memory/                                        │
│  ├── mod.rs          MemoryManager facade                     │
│  ├── store.rs        SQLite storage + markdown fact files     │
│  ├── graph.rs        Rust graph/query layer over SQLite       │
│  ├── search.rs       FTS5 + optional sqlite-vec               │
│  └── learnings.rs    Capture + promote pipeline               │
│                                                               │
│  CLI commands:                                                │
│  codeterm memory save/search/flush/learn/promote              │
│  codeterm memory graph create/query/relate/related/validate   │
│  codeterm skill list/get/search/install/uninstall             │
│                                                               │
│  API: /api/memory/*, socket: memory.*, skill.*                │
└──────────────────────┬────────────────────┬──────────────────┘
                       │                    │
          ┌────────────┴─────┐    ┌────────┴────────────┐
          │  PROVIDER LAYER  │    │  CRON: dreaming      │
          │  (per agent type)│    │  (opt-in, disabled)  │
          │                  │    │                      │
          │  Claude:         │    │  Light → Deep → REM  │
          │   bridge rule    │    │  Graph-aware         │
          │   in ~/.claude/  │    │  consolidation       │
          │   rules/         │    │                      │
          │                  │    │  Schedule:            │
          │  Codex:          │    │  "0 3 * * *"         │
          │   system prompt  │    │  or manual trigger   │
          │   injection      │    │                      │
          │                  │    │                      │
          │  Generic:        │    │                      │
          │   PTY prepend    │    │                      │
          └──────────────────┘    └─────────────────────┘
```

## What's Core vs Plugin

### CORE (always available, part of CLI binary)

| Module | Why core |
|--------|---------|
| **Memory store** (SQLite + markdown fact files + daily notes) | Every agent needs persistence. One local file is easier to query, back up, and evolve. |
| **Knowledge graph** (Rust query layer over SQLite relations) | Infrastructure for skills, plugins, agents, dreaming. Not optional. |
| **Skill broker** (list/get/search/install) | Agents can't use skills without it. CLI is the universal interface. |
| **Learnings capture** (`memory learn`) | Quick capture from any tab. No heavy deps. |

### OPT-IN (disabled by default, enabled via config)

| Module | Why opt-in | Enable |
|--------|-----------|--------|
| **Embeddings / vector search** | Requires sqlite-vec. Heavy for simple use. | `config set memory.embeddings.enabled true` |
| **Dreaming / consolidation** | Creates cron jobs, costs tokens. Not everyone wants it. | `config set memory.dreaming.enabled true` |
| **Provider bridge install** | Installs tiny provider bridge/rule files. Some users may want manual control. | `config set memory.provider_bridge.enabled true` (on by default) |

### NOT A PLUGIN — stays in core binary

The graph and memory store are too fundamental to be plugins:
- Plugins themselves need the graph (to register, to declare relations)
- Memory is needed before any plugin loads
- CLI commands must work without plugin runtime

## Storage Layout

```
~/.codeterm/
  memory/
    global/
      MEMORY.md                      # curated human-readable index, not source of truth
      knowledge.db                   # SQLite source of truth: facts, entities, relations, recall, FTS
      graph-schema.yaml              # type constraints + relations
      facts/
        fact_api_timeout.md          # large promoted facts / narratives / playbooks
      daily/
        2026-04-06.md                # today's notes
        2026-04-05.md                # yesterday's notes
    workspace/{workspace-id}/
      MEMORY.md                      # per-workspace summary/index
      knowledge.db                   # workspace-scoped entities, facts, recall
      facts/
        feature_rollout.md
      daily/
        2026-04-06.md
    .learnings/
      LEARNINGS.md                   # corrections, insights
      ERRORS.md                      # command failures
      FEATURES.md                    # feature requests
    .dreaming/
      short-term-recall.json         # recall frequency tracking
      phase-signals.json             # dreaming phase signals
      consolidation.lock             # prevent concurrent dreaming
  skills/                            # SINGLE SOURCE OF TRUTH
    {skill-name}/
      SKILL.md                       # universal format + metadata header
      scripts/                       # optional helpers
      references/                    # optional deep docs
  plugins/
    {plugin-name}/
      plugin.toml
      skills/                        # plugin-contributed skills
```

## Knowledge Graph — Rust Native, SQLite-Backed

The ontology moves into Rust, but not as a JSONL append log. Source of truth is SQLite.
Rust owns validation, traversal, and query ergonomics. Migration/import from the old `ontology.py`
JSONL format stays supported, but JSONL is not the primary store.

### Data Model

```rust
pub struct Entity {
    pub id: String,              // "{type_prefix}_{uuid_suffix}"
    pub entity_type: String,     // "Task", "Person", "Project", ...
    pub properties: Value,       // serde_json::Value
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub scope: GraphScope,       // global | workspace
}

pub struct Relation {
    pub from_id: String,
    pub relation_type: String,   // "has_task", "assigned_to", ...
    pub to_id: String,
    pub properties: Value,       // optional relation properties
}

```

### Physical Storage

```sql
-- knowledge.db
CREATE TABLE entities (
  id TEXT PRIMARY KEY,
  entity_type TEXT NOT NULL,
  scope TEXT NOT NULL,
  properties_json BLOB NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE relations (
  id INTEGER PRIMARY KEY,
  from_id TEXT NOT NULL,
  relation_type TEXT NOT NULL,
  to_id TEXT NOT NULL,
  scope TEXT NOT NULL,
  properties_json BLOB NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE TABLE memory_items (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,             -- note | learning | fact | rule | summary
  scope TEXT NOT NULL,
  title TEXT,
  body_md_path TEXT,              -- nullable: large facts live as markdown files
  body_inline TEXT,               -- short facts can live inline
  source TEXT,
  created_at TEXT NOT NULL,
  promoted_at TEXT
);

CREATE VIRTUAL TABLE memory_fts USING fts5(
  memory_id UNINDEXED,
  title,
  body,
  tags,
  tokenize = 'unicode61'
);
```

No separate graph database in v1. We keep one local SQLite file and implement graph traversal in
Rust. If traversal becomes hot later, we can add an in-memory adjacency cache without changing the
on-disk format.

### Why SQLite First

- One file for entities, relations, facts, recall signals, and FTS
- Transactions/WAL solve the multi-tab write problem better than ad hoc file locking
- JSON properties stay flexible through `JSON1` / `JSONB`
- `FTS5` gives default BM25 search with no separate search service
- `sqlite-vec` can layer on later without redesigning storage

### Migration

`codeterm memory import-ontology --from graph.jsonl` migrates old append-only ontology data into
`knowledge.db`. Optional export back to JSONL is supported for inspection/debugging, but not used
as the live write path.

### Schema (YAML, same format as existing)

```yaml
types:
  Task:
    required: [title, status]
    properties:
      status:
        enum: [open, in_progress, blocked, done, cancelled]
      priority:
        enum: [low, medium, high, critical]
  Skill:
    required: [name]
    properties:
      path: {}
      description: {}
      triggers: {}
      plugin: {}

relations:
  has_skill:
    from_types: [Plugin, Project]
    to_types: [Skill]
    cardinality: one_to_many
  depends_on:
    from_types: [Task, Skill]
    to_types: [Task, Skill]
    cardinality: many_to_many
    acyclic: true
```

### CLI Interface

```bash
# Entity CRUD
codeterm memory graph create --type Task --props '{"title":"Port graph","status":"open"}'
codeterm memory graph get --id task_abc123
codeterm memory graph list --type Task
codeterm memory graph query --type Task --where '{"status":"open"}'
codeterm memory graph update --id task_abc123 --props '{"status":"done"}'
codeterm memory graph delete --id task_abc123

# Relations
codeterm memory graph relate --from proj_001 --rel has_task --to task_abc123
codeterm memory graph unrelate --from proj_001 --rel has_task --to task_abc123
codeterm memory graph related --id proj_001 --rel has_task
codeterm memory graph related --id task_abc123 --dir incoming

# Schema & validation
codeterm memory graph validate
codeterm memory graph schema-append --data '{"types":{"NewType":{"required":["name"]}}}'
```

## Skill Broker — Single Source of Truth

Skills live in `~/.codeterm/skills/`. No duplication to agent-specific directories.

Each skill is a folder. `SKILL.md` is the canonical file and also carries the skill metadata in its
header. No extra `skill.json` or manifest is required for v1.

Example:

```markdown
---
name: home-assistant
description: Control HA entities, scenes, and automations
tags: [home, automation, zigbee]
requires:
  secrets: [homeassistant.env]
  scripts: [scripts/ha_api.py]
---

# Home Assistant
```

If a legacy skill has no YAML frontmatter yet, broker indexing falls back to:
- folder name as `name`
- first `# Heading` as title
- first paragraph as description

This keeps migration cheap for a 50-200 skill library without inventing another metadata layer.

### How agents access skills

```
Agent (any tab, any provider)
  │
  ├─ codeterm skill list              → compact index (name + description + tags)
  │    Output: "home-assistant — Control HA entities, automations..."
  │            "telegram — Read/send Telegram messages..."
  │            "linear — Manage Linear issues and projects..."
  │
  ├─ codeterm skill preview telegram  → header + key sections + sidecars
  │
  ├─ codeterm skill get telegram      → full SKILL.md content on stdout
  │    Agent reads it, uses it for current task
  │
  └─ codeterm skill search "calendar" → FTS search by description + content
       Returns ranked list of matching skills
```

### Skill Folder Convention

Keep skill-local assets inside the skill folder, not in one giant global `scripts/` directory:

```text
skills/
  home-assistant/
    SKILL.md
    scripts/
    references/
```

This keeps the broker, installer, evaluator, and future plugin migration simple.

### Bridge rule (installed per provider)

The `codeterm.md` bridge rule tells agents about skill access:

```markdown
# ~/.claude/rules/codeterm.md (already exists, extended)

## Skills
You have access to skills via `codeterm skill list`, `codeterm skill preview <name>`, and `codeterm skill get <name>`.
When you need an integration (home automation, APIs, services), check available skills first.
Skills are loaded on-demand — don't load skills you don't need.
```

For Codex: equivalent bridge in Codex's config format.
For generic agents: injected into system prompt at spawn.

The bridge only tells agents how to use the broker. It does not mirror every skill into provider
directories. Small bridge file: yes. Skill duplication: no.

### Skill registry in graph

Skills registered as graph entities:

```bash
codeterm memory graph create --type Skill --id skill_home_assistant \
  --props '{"name":"home-assistant","path":"~/.codeterm/skills/home-assistant","description":"Control HA...","triggers":["home","light","automation"]}'

codeterm memory graph relate --from plugin_ha_bridge --rel has_skill --to skill_home_assistant
```

This lets the graph answer: "which skills does plugin X provide?" and "which skills mention calendar?"

## Learnings Capture

Port of self-improving-agent pipeline. CLI-native, no hooks required.

```bash
# Quick capture from any tab
codeterm memory learn "API timeout fixed by increasing to 30s"
codeterm memory learn --error "sqlite lock timeout on concurrent writes"
codeterm memory learn --feature "need --format json flag on skill list"

# Review
codeterm memory learn list                              # pending learnings
codeterm memory learn list --status resolved             # resolved
codeterm memory learn promote LRN-20260406-001           # → MEMORY.md

# Auto-capture (optional hook in settings.json)
# PostToolUse hook on Bash: runs error-detector, logs to .learnings/ERRORS.md
```

### Learnings → Graph integration

Recurring learnings become graph entities:

```bash
# After 3rd occurrence of same error pattern:
codeterm memory graph create --type Issue --props '{"name":"sqlite lock timeout","status":"open","recurrence":3}'
codeterm memory graph relate --from issue_xxx --rel affects --to skill_memory
```

## Dreaming — Cron-Based Consolidation

Uses existing cron system. Disabled by default.

### Enable

```bash
codeterm config set memory.dreaming.enabled true
# Creates cron job: "0 3 * * *" (3am daily)
# Or custom: codeterm config set memory.dreaming.schedule "0 4 * * *"
```

### Phases (executed by spawned agent)

**Light phase** (fast, cheap):
1. Read today's and yesterday's daily notes
2. Read .learnings/ for new entries
3. Extract candidate entities, relations, facts, and reusable rules
4. Dedup by similarity (semantic if embeddings enabled, keyword otherwise)
5. Stage candidates in SQLite tables / `.dreaming/short-term-recall.json`
6. Never writes directly to `MEMORY.md`

**Deep phase** (balanced):
1. Rank candidates by signals:
   - Frequency (0.24) — how often recalled/referenced
   - Relevance (0.30) — match quality in searches
   - Query diversity (0.15) — referenced from different contexts
   - Recency (0.15) — time-decayed freshness
   - Consolidation (0.10) — multi-day recurrence
   - Graph connectivity (0.06) — connected to many entities
2. Must pass gates: minScore 0.8, minRecallCount 3, minUniqueQueries 3
3. Write structured winners to ontology tables (`entities`, `relations`, `memory_items`)
4. Write vector embeddings for eligible chunks if enabled
5. For large facts/playbooks: create markdown file in `memory/*/facts/`
6. Promote only concise summaries/rules to `MEMORY.md`

**REM phase** (optional, expensive):
1. Pattern extraction across all memories
2. Identify clusters and themes
3. Generate narrative summary (→ optional DREAMS.md)

### Destination Policy

Dreaming should not treat `MEMORY.md` as the main database.

| Output | Goes to | Why |
|--------|---------|-----|
| Entity / relation | SQLite ontology tables | Structured, queryable, dedupable |
| Searchable fact chunk | FTS row + optional vector row | Retrieval path |
| Large narrative / long procedure | `facts/<id>.md` + `memory_items.body_md_path` | Better than bloating the DB or prompt |
| Short durable rule / summary | `MEMORY.md` | Human-readable top layer for GA and fast recall |

### Graph-aware consolidation

Dreaming uses the graph for smarter promotion:

```
"memory about API timeout" 
  → graph: relates to skill_linear, task_api_refactor, person_tima
  → promote as group, not isolated fact
  → `MEMORY.md` entry includes context: "API timeout (affects linear integration, assigned to Tima)"
```

## Compaction & Context Preservation

The critical insight from OpenClaw: flush must happen BEFORE compaction destroys context, not after.

### General Agent (we control the process)

GA runs inside CodeTerm — we own the context lifecycle:
1. **Token threshold** — we track approximate context size. At 70% capacity, inject a silent "save what matters" turn before continuing. No external hooks needed.
2. **Proactive periodic save** — GA saves to daily notes every N turns (configurable), not just at compaction.
3. **Session end** — always flush on GA shutdown/restart.

This is the simplest case: we are the orchestrator, we decide when to flush.

### Regular tabs (Claude Code)

Agent runs in a PTY, we observe but don't control:
1. **PreCompact hook** (preferred): fires BEFORE context is summarized
   ```json
   {"hooks": {"PreCompact": [{"type": "command", "command": "codeterm memory flush --pane $CODETERM_PANE_ID"}]}}
   ```
2. **PTY detection** (fallback): detect "conversation compressed" / compaction markers in terminal output — at this point context is already lost, but we can still save the raw PTY buffer
3. **Bridge rule teaches proactive saves** — agent periodically calls `codeterm memory save "..."` itself, not waiting for compaction

### Codex / Other agents

1. **PTY detection**: agent-specific compaction markers
2. **Bridge rule**: inject `codeterm memory save` pattern into agent instructions
3. **Session end**: always flush on pane exit (pane lifecycle hook, already exists)

### Flush: two-stage approach

**Stage 1 — Agent-side (proactive, before compaction):**
The agent itself decides what's important and calls `codeterm memory save "decided to use X because Y"`. Bridge rule teaches this. High quality, low noise.

**Stage 2 — System-side (safety net, on pane exit or compaction):**
```bash
codeterm memory flush --pane PANE_ID
# 1. Grab raw PTY buffer (full scrollback)
# 2. Store as-is in daily/YYYY-MM-DD-pane-{id}.raw (not in daily notes)
# 3. Dreaming phase later extracts value from raw dumps
# 4. Record recall signals for dreaming
```
Raw output is NOT promoted to daily notes directly — it's noisy. Dreaming does the curation.

## Provider Sync

How memories and skills reach each agent type.

### Claude

```
Bridge: ~/.claude/rules/codeterm.md (already managed by CodeTerm)
  - Updated on: CodeTerm start, config change, plugin install/uninstall
  - Contains: compact skill index, broker usage, memory commands

Skills:
  - NOT duplicated to ~/.claude/skills/
  - Agent uses `codeterm skill list/preview/get/search`
  - Optional explicit export shortcut can exist later, but not as canonical delivery

Memory:
  - NOT duplicated to provider directories
  - Agent uses `codeterm memory search`, `codeterm memory fact read`, `codeterm memory graph ...`
```

### Codex

```
Bridge: installed via Codex config / prompt bridge
  - Same logical content as Claude bridge, different delivery

Skills: same broker pattern — `codeterm skill list/preview/get`
Memory: same broker pattern — `codeterm memory search/query/fact read`
```

### General Agent (special)

```
Gets everything at spawn time:
  - Full skill index (descriptions) in system prompt
  - Memory `MEMORY.md` summary in system prompt
  - Graph entities relevant to current workspace
  - All CLI commands available

General Agent is the exception: it is not told to mirror skills or to behave like a consumer of a
provider-native skill folder. It already owns the canonical source of truth.
```

## Tab Awareness

### General Agent tab

- Knows ALL skills, ALL memory, full graph access
- Can delegate tasks to worker tabs with relevant context
- Receives reports from workers, updates memory/graph

### Regular tabs (Claude/Codex)

- Know about skills via bridge rule (`codeterm skill list/get`)
- Can save/search memory via CLI
- Can read/write graph via CLI
- Don't get full skill index at start — load on demand

### Worker tabs (spawned by orchestrator)

- Get task-specific context from parent
- Can use `codeterm memory learn` to capture learnings
- Report back via `codeterm agent report`
- Minimal skill knowledge (only what's needed for task)

## Config Schema

```toml
[memory]
enabled = true                               # master switch

[memory.store]
global_dir = "~/.codeterm/memory/global"     # long-term storage
daily_retention_days = 90                    # auto-cleanup old daily notes

[memory.graph]
enabled = true                               # graph engine (lightweight, always on)
schema_path = "~/.codeterm/memory/global/graph-schema.yaml"

[memory.skill_broker]
enabled = true
compact_index_limit = 200                    # no need to overcomplicate for a 50-200 skill library
fallback_parse_legacy_headers = true         # old SKILL.md files without frontmatter still work

[memory.embeddings]
enabled = false                              # opt-in: requires sqlite-vec
provider = "local"                           # "local" (sqlite-vec) | "openai" | "voyage"
model = "all-MiniLM-L6-v2"                  # local embedding model
dimension = 384

[memory.dreaming]
enabled = false                              # opt-in: cron-based consolidation
schedule = "0 3 * * *"                       # cron expression
agent = "claude"                             # which agent runs dreaming
phases = ["light", "deep"]                   # which phases to run (REM optional)

[memory.learnings]
auto_capture = false                         # hook-based error detection
promote_threshold = 3                        # recurrence count before auto-promote

[memory.provider_bridge]
enabled = true                               # install/update bridge rules to providers
providers = ["claude", "codex"]             # which providers to sync

[memory.mesh_sync]
enabled = false                              # opt-in replication across trusted mesh peers
mode = "manual"                              # "manual" | "scheduled" | "eventual"
replicate = ["skills", "schema", "curated_memory"]
max_graph_nodes = 10000
max_vector_docs = 15000
```

## Implementation Phases

### Phase 3.1: Memory Store + Graph Engine (1 week)

- [ ] `src-tauri/src/memory/mod.rs` — MemoryManager facade
- [ ] `src-tauri/src/memory/store.rs` — SQLite + fact markdown files + daily notes
- [ ] `src-tauri/src/memory/graph.rs` — Rust-native ontology over SQLite (schema, validation)
- [ ] CLI: `codeterm memory save`, `codeterm memory daily`, `codeterm memory graph *`
- [ ] API: REST + Socket + dispatch integration
- [ ] Config: `[memory]` and `[memory.graph]` sections
- [ ] Legacy importer: JSONL ontology → SQLite
- [ ] Tests: unit tests for graph operations, SQLite round-trips, schema validation

### Phase 3.2: Skill Broker (1 week)

- [ ] `src-tauri/src/memory/skills.rs` — skill discovery, indexing, search
- [ ] CLI: `codeterm skill list`, `codeterm skill preview`, `codeterm skill get`, `codeterm skill search`
- [ ] CLI: `codeterm skill install <path>`, `codeterm skill uninstall <name>`
- [ ] Parse skill metadata from `SKILL.md` header, no extra manifest
- [ ] Skill registry in graph (Skill entities + relations)
- [ ] Bridge rule generation for Claude/Codex without skill mirroring
- [ ] Plugin skill contribution: plugins declare skills in manifest, auto-registered
- [ ] Bundle starter skills: `codeterm`, `ontology`, `skill-creator`, `skill-evaluator`, `self-improving-agent`

### Phase 3.3: Learnings + Compaction (1 week)

- [ ] `src-tauri/src/memory/learnings.rs` — capture, categorize, promote
- [ ] CLI: `codeterm memory learn`, `codeterm memory learn list/promote`
- [ ] `codeterm memory flush --pane ID` — pre-compaction save
- [ ] Claude hook: PreCompact → `codeterm memory flush`
- [ ] PTY detection: compaction markers for Claude/Codex
- [ ] Pane lifecycle: auto-flush on agent exit

### Phase 3.4: Dreaming (1 week)

- [ ] Cron job creation on `config set memory.dreaming.enabled true`
- [ ] Dreaming agent task prompt (light + deep phases)
- [ ] Graph-aware consolidation (group related memories)
- [ ] Short-term recall tracking (`.dreaming/short-term-recall.json`)
- [ ] Structured writes first: ontology rows + fact files + optional vectors
- [ ] `MEMORY.md` promotion only for concise summaries/rules
- [ ] Mesh-aware replication policy for curated outputs

### Phase 3.5: Embeddings / Vector Search (optional, 1 week)

- [ ] SQLite + sqlite-vec integration
- [ ] Hybrid search: vector similarity + BM25 + temporal decay
- [ ] MMR re-ranking for diversity
- [ ] Embedding providers: local (all-MiniLM) or API (OpenAI, Voyage)
- [ ] Index: memory files + skill files + graph entities
- [ ] CLI: `codeterm memory search "query"` enhanced with semantic search

## Migration Path for Existing Skills

User's 44 skills currently in `~/Desktop/clawd-skills-scripts/skills/`:

```bash
# One-time import:
codeterm skill install ~/Desktop/clawd-skills-scripts/skills/home-assistant
codeterm skill install ~/Desktop/clawd-skills-scripts/skills/telegram
# ... or batch:
for dir in ~/Desktop/clawd-skills-scripts/skills/*/; do
  codeterm skill install "$dir"
done
```

This copies the whole skill folder to `~/.codeterm/skills/{name}/`, preserving local `scripts/`
and `references/`, then registers it in the broker/graph.

Skills that become plugins (home-assistant, telegram, etc.) will later be migrated:
```bash
# Plugin subsumes the skill:
codeterm plugin install ha-bridge
# Plugin manifest declares its skills → auto-registered
# Old standalone skill can be uninstalled:
codeterm skill uninstall home-assistant
```

## Hardening (from architecture review)

### Concurrency: all writes through MemoryManager

Multiple tabs call `codeterm memory graph create` simultaneously. Direct file writes or ad hoc
per-command SQLite access will race.

**Solution:** CLI commands hit REST/Socket API → MemoryManager → transaction queue. Same pattern as
multiplexer state. SQLite runs in WAL mode; writes use short explicit transactions.

### Crash recovery: SQLite + atomic markdown writes

Power loss during a write should not corrupt the ontology or fact files.

**Solution:**
- SQLite source of truth uses WAL mode + transactional writes
- Large fact markdown files write to `*.tmp` and rename atomically
- Periodic `VACUUM` / `ANALYZE` maintenance replaces JSONL compaction as the file-health task

### Graph maintenance

At scale, maintenance is DB-oriented, not append-log-oriented.

```bash
codeterm memory graph maintain   # ANALYZE/VACUUM/rebuild derived indexes
```

Automatic maintenance can trigger on startup or when DB/FTS growth crosses thresholds.

### Disk quotas

```toml
[memory.limits]
graph_max_mb = 100                # reject writes when exceeded
daily_max_files = 365             # auto-cleanup oldest beyond this
learnings_max_entries = 1000      # auto-archive oldest
embeddings_max_mb = 500           # sqlite-vec DB size cap
```

Warning at 80% threshold. Hard reject at 100% with clear error message.

### Schema validation on every write

Schema validation is NOT optional — runs on every `create` and `update`. The `validate` command is for bulk-checking existing data after schema changes.

New required fields added to schema get defaults for existing entities. `schema_version` field tracks schema evolution.

### Security

- `~/.codeterm/memory/` created with `0700` permissions (Unix)
- Windows: NTFS ACL for current user only
- `memory.sensitive_filter` option: regex patterns to redact before persisting (API keys, tokens)
- Memory is LOCAL ONLY by default. Mesh sync requires explicit `memory.mesh_sync.enabled = true` with per-category controls (sync skills yes, sync graph no, sync daily no)
- Dreaming agent runs with reduced CLI access (read-only graph except promote ops)

### Mesh replication policy

Mesh is useful here, but replication must be selective and bounded.

Default:
- Replicate: skills, graph schema, curated memory summaries
- Optional: graph/entity deltas, vector chunks, dreaming signals
- Never replicate: secrets, raw PTY dumps, provider auth state, local `.learnings/` noise unless explicitly exported

Policy knobs:
- `mode = manual` for small/home setups
- `mode = scheduled` for nightly or low-churn replication
- `mode = eventual` only for low-latency trusted peers and bounded datasets

Capacity assumptions for now:
- Graph target budget: up to ~10k nodes is comfortable on-device
- Vector budget: up to ~15k documents/chunks is acceptable as a high-end ceiling
- If datasets grow beyond that, replication should switch to summaries/deltas, not full eager copies

### Skill discovery: compact index in bridge rule

Without ambient skill awareness, agents forget skills exist. Regression from Claude Code's native skill descriptions.

**Solution:** Bridge rule includes compact skill index:
```markdown
## Available Skills
home-assistant — Control HA entities, automations, scenes
telegram — Read/send Telegram messages, manage channels
linear — Manage Linear issues, projects, cycles
...
Use `codeterm skill get <name>` for full instructions.
```

~660 tokens for 44 skills. Regenerated on skill install/uninstall. Agents see what's available without running `skill list`.

### Search quality without embeddings

Keyword grep is useless for semantic recall. Default search must be BM25 minimum.

**Solution:** SQLite `FTS5` is the default search layer. Tags on save:
```bash
codeterm memory save --tags "api,timeout,fix" "increased deadline to 30s"
```
Tags give keyword search a fighting chance. Embeddings via `sqlite-vec` enhance but aren't required.

### Cross-tab awareness

Tab B doesn't know Tab A saved something.

**Solution:** Memory write events broadcast via existing socket event bus:
```rust
SystemEvent::MemoryChanged { category: "daily", key: "2026-04-06", pane_id: "..." }
```
Agents in same workspace can subscribe. CLI:
```bash
codeterm memory diff --since "10m ago"    # what changed recently
```

### Graph query ergonomics

JSON-in-shell-quotes is error-prone.

**Solution:** Key-value sugar for common queries:
```bash
codeterm memory graph list --type Task --status open     # sugar
codeterm memory graph query --type Task --where '{"status":"open"}'  # full power
codeterm memory graph stats                               # entity counts by type
```

### Cold start

First time, everything empty. Agent tries commands, gets nothing, stops trying.

**Solution:**
- `codeterm memory init` — creates dirs, seeds with bootstrap entities (user, machine)
- Empty `skill list` prints: "No skills installed. Import: `codeterm skill install <path>`"
- Empty `memory search` prints: "No memories yet. Save with: `codeterm memory save \"...\"`"
- First CodeTerm start auto-runs `memory init`

### Error messages: actionable, not cryptic

All memory/graph CLI commands return structured errors:
```
Error: Schema validation failed for type "Task"
  Missing required field: "status"
  Valid values: open, in_progress, blocked, done, cancelled
  Fix: add --props '{"title":"...","status":"open"}'
```

Empty search results include suggestions, not silence.

## Cron Hardening for Agent Tasks

Periodic agent invocations need resilience and organic patterns.

### Jitter

```toml
# Per cron job:
jitter_max_ms = 900000    # ±15 minutes (default for agent-spawning crons)
```

Applied in `compute_next_run`: `next_run += random(0..jitter_max_ms)`. Default = 10% of interval, min 1 minute, max 30 minutes.

### Prompt uniqueness

Each cron run prepends dynamic context:
```
Run context: job={job_id} | run={run_id} | time={utc_now}
Recent: {last_git_commit_summary} | {system_load_summary}
```
Combined with existing `CRON_COMPLETE_SUFFIX` (unique run_id), every prompt is unique.

### Failure tracking and backoff

```rust
pub struct CronJob {
    // ... existing fields ...
    pub consecutive_failures: u32,
    pub jitter_max_ms: Option<u64>,
}
```

On failure: increment `consecutive_failures`, push `next_run` by exponential backoff (`base_ms * 2^failures`, capped at 1 hour, with 25% jitter). After 5 consecutive failures: auto-disable, notify user. On success: reset counter.

### Token budget for dreaming

Dreaming cron job has a configurable token cap. If the agent exceeds it, `codeterm cron complete` is forced. Prevents runaway consolidation costs.

```toml
[memory.dreaming]
max_tokens_per_run = 50000    # approximate budget
```

## Key Decisions

1. **Graph + Memory Store = CORE**, not plugin. Too fundamental. Plugins depend on them.
2. **SQLite is the source of truth.** `MEMORY.md` is a curated human-readable index, not the main database.
3. **Skills accessed via CLI**, not file duplication. One source, any agent.
4. **Skill metadata lives in `SKILL.md` header.** No extra skill manifest for a 50-200 skill library.
5. **Skills are folders with local scripts/references.** Avoid one giant global scripts directory.
6. **Bridge rule with compact skill index** — agents see what's available. Full content on-demand via broker.
7. **Dreaming writes structured data first** (SQLite ontology / FTS / vectors / fact files), then promotes concise summaries to `MEMORY.md`.
8. **All writes through MemoryManager** (API-mediated). CLI never writes files directly.
9. **Pre-compaction flush for GA** (we control context). PreCompact hook for Claude tabs. Agent-side proactive saves via bridge rule.
10. **Memory is local-first.** Mesh sync is explicit, selective, and never includes secrets by default.
11. **Schema validation on every write.** Not optional, not just a check command.
12. **Cron jitter + prompt uniqueness + backoff** for resilient periodic tasks.
13. **Bundled starter skills ship with CodeTerm.** At minimum: `codeterm`, `ontology`, `skill-creator`, `skill-evaluator`, `self-improving-agent`.
14. **Dreaming disabled by default.** Opt-in via config. Uses existing cron system.
