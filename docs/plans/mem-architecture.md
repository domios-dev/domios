# Mem — CodeTerm Unified Memory System

**Status:** Research complete, architecture locked, implementation not started
**Date:** 2026-04-06 (revised 2026-04-08: unified skills+memory model, MemPalace port notes, `mem` naming)
**Parent:** `docs/plans/plugin-architecture.md` Phase 3 — this is the detailed design.
**Name:** `mem` throughout (CLI, Rust module, config key, storage dir). NOT `memory`, NOT `palace`. Short, matches CodeTerm language style (`pane`, `ws`, `mem`).

## Problem

Agents (Claude, Codex, any provider) running in CodeTerm tabs have no persistent memory across sessions. Each session starts blank. Worse — CodeTerm today has **two separate would-be systems** for persistent data:

1. **Skills** (`~/.codeterm/skills/<name>/SKILL.md`) — structured instructions the agent can lazy-load
2. **Memory** (planned SQLite store with entities / facts / notes)

These are **the same thing**. A skill is a memory entry the agent pulls in when relevant. A learned fact is a memory entry the agent pulls in when relevant. A conversation excerpt is a memory entry the agent pulls in when relevant. Splitting them into two systems means two CLIs for agents to learn (`codeterm skill get` + `codeterm mem search`), two storage layouts, two lookup hierarchies, two bridge rule payloads, two places where things can get out of sync, and no way to do cross-type queries ("find everything about home automation — skills, past conversations, scripts I wrote, preferences").

**Unified model:** skills, facts, conversation logs, scripts, peer nicknames, project locations, user preferences — all live in **one** `mem_items` table discriminated by a `kind` column. Agents query **one** CLI (`codeterm mem`) and get back whatever matches, whether it's a skill instructions file, a daily note fragment, or a graph-extracted fact. The bridge rule teaches **one** thing: "use `codeterm mem search/get/recall` for everything CodeTerm knows."

Key questions this plan answers:
1. Where do mem items live and how are they searched?
2. How do skills (as a kind of mem item) get delivered to any agent without duplication?
3. How does the temporal knowledge graph integrate?
4. How does compaction/consolidation work?
5. Why port MemPalace's ideas but not their code?

## TL;DR

- **One SQLite file** at `~/.codeterm/mem/knowledge.db` — entities, triples (with valid_from/valid_to), mem_items, FTS5, optional sqlite-vec vectors. Backup = copy one file.
- **Everything is a mem_item.** `kind ∈ {skill, fact, note, conversation, script, peer, preference, daily, learning, rule}`. No separate skills directory, no separate memory store.
- **`codeterm mem`** is the single agent-facing CLI. `search`, `get`, `save`, `recall`, `graph *`, `wake-up`, `learn`, `flush`.
- **Layered stack** (port from MemPalace): L0 identity (~100 tok, always loaded), L1 critical facts (~500 tok, always loaded), L2 on-demand workspace/area recall, L3 deep semantic search (embeddings optional).
- **Bridge rule per provider** installs a one-page instruction teaching `codeterm mem *`. No pre-built skill index — agents query on demand.
- **Dreaming** (cron-based consolidation) writes alongside verbatim, never replacing raw.
- **mem is CORE** — the graph and memory store are too fundamental to be a plugin (plugins depend on them).
- **No Python, no ChromaDB** — fully Rust, fully SQLite. Port MemPalace's _ideas_ from their code (see `references/mempalace/`); reject their implementation choices.

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
| [MemPalace](https://github.com/milla-jovovich/mempalace) ([local notes](references/mempalace.md)) | Verbatim-first storage beats summaries on LongMemEval (96.6% raw mode). Hierarchical metadata filtering (wing/room ≈ workspace/area) gives ~34% retrieval boost, but it's just SQL `WHERE` — no new vector magic. Temporal validity windows on graph facts for as-of queries. Reject: AAAK dialect (maintainers' own correction says it regresses), ChromaDB (we stay SQLite-only), verbatim-only purism (we keep dreaming as complement). |

## Architecture Overview

```
┌──────────────────────────────────────────────────────────────┐
│                        CORE (src-tauri)                       │
│                                                               │
│  src-tauri/src/mem/                                           │
│  ├── mod.rs          MemManager facade — API-mediated writes  │
│  ├── store.rs        SQLite connection pool, mem_items CRUD   │
│  ├── graph.rs        Entities + triples, temporal traversal   │
│  ├── search.rs       FTS5 (default) + sqlite-vec (opt-in)     │
│  ├── layers.rs       L0/L1/L2/L3 wake-up stack                │
│  ├── dreaming.rs     Consolidation phases (opt-in cron)       │
│  ├── learnings.rs    Capture + promote pipeline               │
│  └── bridge.rs       Per-provider bridge rule install         │
│                                                               │
│  Dispatch commands (unified with Chunk B):                    │
│   MemSearch { query, kind?, scope?, limit }                   │
│   MemGet { id }                                                │
│   MemSave { kind, title, body, tags?, scope? }                │
│   MemRecall { query, budget_tokens? }                         │
│   MemGraphCreate / GraphRelate / GraphQuery / GraphRelated    │
│   MemWakeUp { budget_tokens }                                 │
│   MemLearn { text, kind? }                                    │
│                                                               │
│  CLI (wraps dispatch):                                        │
│   codeterm mem search/get/save/recall/wake-up/learn/flush     │
│   codeterm mem graph create/relate/query/related/validate    │
│                                                               │
│  (NO `codeterm skill *` — skills are mem_items with kind=skill)│
│                                                               │
│  API: /api/mem/*  Socket: mem.*  Tauri: mem_*                 │
└──────────────────────┬────────────────────┬──────────────────┘
                       │                    │
          ┌────────────┴─────┐    ┌────────┴────────────┐
          │ BRIDGE RULES     │    │  CRON: dreaming      │
          │ (per agent type) │    │  (opt-in, disabled)  │
          │                  │    │                      │
          │ Claude:          │    │  Light → Deep → REM  │
          │  ~/.claude/      │    │  Graph-aware         │
          │  rules/          │    │  consolidation       │
          │  codeterm.md     │    │  writes alongside    │
          │  teaches         │    │  verbatim, never     │
          │  `codeterm mem`  │    │  replacing raw       │
          │                  │    │                      │
          │ Codex:           │    │  Schedule:            │
          │  AGENTS.md       │    │  "0 3 * * *"         │
          │  inline section  │    │  or manual trigger   │
          │                  │    │                      │
          │ Generic:         │    │                      │
          │  system prompt   │    │                      │
          │  injection       │    │                      │
          └──────────────────┘    └─────────────────────┘
```

## What's Core vs Plugin

### CORE (always available, part of binary)

| Module | Why core |
|--------|---------|
| **Mem store** (SQLite with `mem_items` + graph + FTS5) | Every agent needs persistence. One local file is easier to query, back up, and evolve. Plugins depend on it (they register their own mem_items), so it must exist before plugins load. |
| **Knowledge graph** (entities + triples with temporal validity) | Infrastructure for skills, plugins, agents, dreaming. Not optional. |
| **Unified search / get / save / recall** | Agents can't use anything without this. CLI is the universal interface — one command surface instead of separate `skill *` + `memory *`. |
| **Bridge rule installer** | Each active agent provider gets a rule file / AGENTS.md section teaching `codeterm mem *`. Written by the core on startup + on `contributions-changed` events. |
| **Layered wake-up** (L0-L3) | GA init flow depends on `codeterm mem wake-up` for bootstrap context. Part of core because the GA is in core. |
| **Learnings capture** (`codeterm mem learn`) | Quick capture from any tab. No heavy deps. |

### OPT-IN (disabled by default, enabled via config)

| Module | Why opt-in | Enable |
|--------|-----------|--------|
| **Embeddings / vector search** | Requires sqlite-vec + a local embedding model (or external API). Heavy for minimal use; FTS5 works without it. | `config set mem.embeddings.enabled true` |
| **Dreaming / consolidation** | Creates cron jobs, costs tokens. Not everyone wants it. | `config set mem.dreaming.enabled true` |
| **Mesh replication** | Cross-device mem sync is selective (curated only, never raw/secrets) and opt-in per category. | `config set mem.mesh_sync.enabled true` |

### NOT A PLUGIN — stays in core binary

The graph and mem store are too fundamental to be a plugin:
- Plugins themselves need the graph (to register their own entities, declare relations, contribute mem_items)
- Mem is needed before any plugin loads
- CLI commands must work without plugin runtime
- All agent providers need the bridge rule mechanism which lives in core

**This is our biggest architectural difference from MemPalace**, which is a standalone app. For us, mem is infrastructure that everything else builds on.

## Storage Layout

**Single unified tree — one `mem/` directory, no separate `skills/`:**

```
~/.codeterm/
  mem/
    knowledge.db                     # SQLite source of truth — one file for
                                     # everything: entities, triples, mem_items
                                     # (all kinds), FTS5, optional sqlite-vec
                                     # Backup = copy this file.
    graph-schema.yaml                # type constraints + relations declaration
    blobs/                           # large inline bodies content-addressed by hash
      sha256_abc123.md               # e.g. a SKILL.md body, a long daily note,
      sha256_def456.py               # a script file, a full conversation log
    daily/                           # daily note scratch (human-readable)
      2026-04-08.md
      2026-04-07.md
    .learnings/                      # pending learnings before promotion
      LEARNINGS.md                   # corrections, insights
      ERRORS.md                      # command failures
      FEATURES.md                    # feature requests
    .dreaming/                       # runtime state for the dreaming cron
      short-term-recall.json         # recall frequency tracking
      phase-signals.json             # dreaming phase signals
      consolidation.lock             # prevent concurrent dreaming runs

  general/                           # GA working directory (already exists)
    MEMORY.md                        # curated human-readable index — the L0+L1
                                     # wake-up output gets saved here for the GA's
                                     # own reference. Sourced from knowledge.db,
                                     # not the other way around.
    bootstrap.md                     # first-run bootstrap (already exists)
    AGENTS.md                        # Codex's inline view (already regenerated)
```

**What's NOT in the tree anymore:**
- ~~`~/.codeterm/skills/<name>/SKILL.md`~~ — replaced by `mem_items` rows with `kind = 'skill'`. SKILL.md body content lives in `blobs/<hash>.md` referenced by `mem_items.body_blob_path`.
- ~~`~/.codeterm/memory/global/` vs `~/.codeterm/memory/workspace/<id>/`~~ — one `knowledge.db` for everything; scope is a column on mem_items (`"global"` vs `"workspace:<id>"`), not a separate directory.
- ~~`plugins/<name>/skills/`~~ — plugins contribute mem_items at activation via `Plugin::mem_items() -> Vec<MemItem>`; no separate filesystem slot.

**Scope semantics:**
- `scope = 'global'` — available everywhere
- `scope = 'workspace:<id>'` — only surfaces when the query is scoped to that workspace
- Queries without a scope filter see both (global always, current workspace if one is active)

**Content storage:**
- Short bodies (<2KB) → `mem_items.body_inline TEXT`
- Large bodies (skill SKILL.md, long notes, scripts, raw conversation logs) → `blobs/sha256_<hash>.<ext>` + `mem_items.body_blob_path` pointer
- Content-addressed blobs deduplicate automatically (two mem_items referencing the same script share one file)
- Blob deletion is refcounted — a blob gets removed when no mem_item references it

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

-- Typed relationships with temporal validity (port from MemPalace knowledge_graph.py).
-- `valid_from` + `valid_to` let us answer "what was true about X as of <date>?"
-- and auto-filter stale facts from recall. Null `valid_to` = still valid.
CREATE TABLE relations (
  id INTEGER PRIMARY KEY,
  from_id TEXT NOT NULL,
  relation_type TEXT NOT NULL,     -- has_skill, depends_on, assigned_to, works_on, ...
  to_id TEXT NOT NULL,
  scope TEXT NOT NULL,
  valid_from TEXT NOT NULL,        -- ISO 8601 timestamp
  valid_to TEXT,                   -- nullable; null = still valid
  confidence REAL DEFAULT 1.0,
  source_mem_id TEXT,              -- link back to the mem_item this fact was extracted from
  properties_json BLOB NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (from_id) REFERENCES entities(id),
  FOREIGN KEY (to_id) REFERENCES entities(id),
  FOREIGN KEY (source_mem_id) REFERENCES mem_items(id)
);

CREATE INDEX idx_relations_from ON relations(from_id);
CREATE INDEX idx_relations_to ON relations(to_id);
CREATE INDEX idx_relations_type ON relations(relation_type);
CREATE INDEX idx_relations_valid ON relations(valid_from, valid_to);

-- The ONE table for everything the agent can query — skills, facts, notes,
-- conversations, scripts, peers, preferences. `kind` discriminates. Same
-- filtering + FTS + (optional) vector search works across all kinds.
CREATE TABLE mem_items (
  id TEXT PRIMARY KEY,             -- "{kind}_{uuid_suffix}" e.g. skill_home_assistant
  kind TEXT NOT NULL,              -- skill | fact | note | conversation | script |
                                   -- peer | preference | daily | learning | rule | summary
  scope TEXT NOT NULL,             -- 'global' | 'workspace:<id>'
  title TEXT NOT NULL,             -- human name: "home-assistant", "api-timeout-fix"
  summary TEXT,                    -- short description (~50-200 chars) — L2 preview
  area TEXT,                       -- optional metadata filter tag (~ MemPalace "room"):
                                   -- auth | billing | deploy | home | automation | ...
  tags_json TEXT DEFAULT '[]',     -- ["home", "automation"] — used by search
  body_inline TEXT,                -- short bodies (<2KB) live inline
  body_blob_path TEXT,             -- nullable: path to blobs/<hash> for large bodies
  source TEXT,                     -- where it came from: file path, pane id, plugin id
  importance REAL DEFAULT 3.0,     -- 1-5; drives L1 wake-up ranking
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  accessed_at TEXT                 -- bumps on each `mem get` / `mem search` hit
);

CREATE INDEX idx_mem_kind ON mem_items(kind);
CREATE INDEX idx_mem_scope ON mem_items(scope);
CREATE INDEX idx_mem_area ON mem_items(area);
CREATE INDEX idx_mem_importance ON mem_items(importance DESC);

CREATE VIRTUAL TABLE mem_fts USING fts5(
  mem_id UNINDEXED,
  title,
  summary,
  body,                            -- full body text extracted from inline or blob
  tags,
  tokenize = 'unicode61'
);

-- Optional vector layer (Phase 3.5, sqlite-vec extension).
-- Rows are 1:1 with mem_items but only populated when embeddings are enabled.
-- CREATE VIRTUAL TABLE mem_vectors USING vec0(
--   id TEXT PRIMARY KEY,
--   embedding FLOAT[384]           -- all-MiniLM-L6-v2 default; configurable
-- );
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

`codeterm mem import-ontology --from graph.jsonl` migrates old append-only ontology data into
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
# ── Unified mem CLI (replaces `codeterm mem *` AND `codeterm skill *`) ──

# Search — full-text + (optional) vector across ALL kinds
codeterm mem search "home automation"            # returns ranked mem_items
codeterm mem search "sqlite timeout" --kind fact # filter by kind
codeterm mem search "auth" --area billing        # metadata filter
codeterm mem search "deploy" --scope workspace   # current workspace only

# Get by id
codeterm mem get skill_home_assistant            # prints full body (inline or blob)
codeterm mem get fact_api_timeout_fix

# Save (manual capture from any tab)
codeterm mem save --kind fact "API timeout fixed by bumping deadline to 30s"
codeterm mem save --kind learning --tags auth,jwt "JWT refresh needs the kid header"
codeterm mem save --kind skill --title weather-api --body "$(cat skills/weather.md)"

# Recall — semantic-first (uses vectors if enabled, FTS5 otherwise)
codeterm mem recall "how do I check HA automation state"
codeterm mem recall "what was the last deploy issue" --limit 3

# Wake-up — prints L0+L1 context for GA init injection
codeterm mem wake-up                             # ~600-900 tokens; what GA loads
codeterm mem wake-up --budget 200                # tighter budget

# Learnings capture pipeline
codeterm mem learn "API timeout fixed by increasing deadline to 30s"
codeterm mem learn --error "sqlite lock timeout on concurrent writes"
codeterm mem learn --feature "need --format json flag on mem search"
codeterm mem learn list                          # pending learnings
codeterm mem learn promote LRN-20260408-001      # → mem_item kind=fact

# Pre-compact flush (called from Claude PreCompact hook)
codeterm mem flush --pane PANE_ID                # grabs PTY buffer, stores as raw note

# ── Graph subcommands (entities + temporal triples) ──

codeterm mem graph create --type Task --props '{"title":"Port graph","status":"open"}'
codeterm mem graph get --id task_abc123
codeterm mem graph list --type Task
codeterm mem graph query --type Task --where '{"status":"open"}'
codeterm mem graph query --type Task --as-of 2026-04-01  # temporal query
codeterm mem graph update --id task_abc123 --props '{"status":"done"}'
codeterm mem graph delete --id task_abc123

# Relations with validity windows
codeterm mem graph relate --from person_kai --rel works_on --to project_orion \
  --valid-from 2026-06-01
codeterm mem graph relate --from max --rel has_issue --to sports_injury \
  --valid-from 2026-01-15 --valid-to 2026-02-20     # already invalidated
codeterm mem graph unrelate --from proj_001 --rel has_task --to task_abc123
codeterm mem graph related --id person_kai --rel works_on
codeterm mem graph related --id task_abc123 --dir incoming --as-of 2026-03-01

# Schema & validation
codeterm mem graph validate
codeterm mem graph schema-append --data '{"types":{"NewType":{"required":["name"]}}}'

# Maintenance
codeterm mem graph stats                         # entity/triple counts by type
codeterm mem graph maintain                      # VACUUM + ANALYZE + rebuild indexes
```

**Note:** `codeterm skill *` does **not** exist. Skills are mem_items with `kind = 'skill'`; use `codeterm mem search --kind skill` and `codeterm mem get skill_<name>`.

## Skills as mem_items — the unification

Skills are just mem_items with `kind = 'skill'`, same query path as everything else. No separate broker, no separate CLI, no separate lookup hierarchy.

### Rules vs skills vs mem

Claude has two distinct provider mechanisms that are easy to conflate, and mem sits underneath both:

| Mechanism | Path | Lifecycle | Purpose |
|-----------|------|-----------|---------|
| **Claude rules** | `~/.claude/rules/*.md` | Eager — injected into every session's system prompt | Always-active instructions the agent must remember |
| **Claude skills** | `~/.claude/skills/<name>/SKILL.md` | Lazy — auto-discovered by Claude's Skill tool when trigger words match | Reference material the agent loads on demand |
| **CodeTerm mem** | `knowledge.db` (one SQLite file) | Lazy, on-demand via `codeterm mem *` CLI | Everything CodeTerm knows — skills, facts, notes, scripts, etc |

**Our bridge rule lives in `~/.claude/rules/codeterm.md`** — a Claude rule, not a skill. It's always-on, contains one page of instructions teaching the agent the `codeterm mem *` CLI. It does NOT list available skills/mem_items by name — that's fetched on demand via `codeterm mem search`.

**We never write to `~/.claude/skills/` or `~/.agents/skills/`.** Those are user territory. If the user has their own Claude skills installed there, we don't touch them. Our mem_items live in `knowledge.db`; Claude's native skills live in `~/.claude/skills/`. Two separate universes.

**Codex** has no native rule/skill slot; its bridge equivalent is inlined into `AGENTS.md` (which CodeTerm already regenerates from `general/MEMORY.md` and the `CODETERM_RULE` template). We add the mem CLI instructions to the same AGENTS.md assembly.

**Generic / local LLMs** receive the bridge content prepended to their system prompt at spawn time via `AgentProvider::deliver_prompt()`.

### mem_item kinds and their scopes

```rust
pub enum MemKind {
    Skill,         // long-form instructions (was SKILL.md files)
    Fact,          // promoted learnings — "X is true because Y"
    Note,          // short contextual note — "remember we tried approach X"
    Conversation,  // extracted excerpt from an agent session
    Script,        // executable code snippet (path to blob = actual script)
    Peer,          // mesh peer metadata — aliases, machine role, capabilities
    Preference,    // user preferences — theme, tone, language, tools
    Daily,         // a chunk of today's daily notes
    Learning,      // pending learning before promotion (pre-dreaming)
    Rule,          // always-on always-true fact ("never use `echo` for structured text")
    Summary,       // dreaming-generated consolidation of many items
}
```

Each kind uses the same `mem_items` columns — `title`, `summary`, `body_inline`/`body_blob_path`, `tags_json`, `importance`, `scope`, `area`. Kind affects only filter defaults (e.g. `codeterm mem wake-up` skips `kind = 'conversation'` because those are noisy; `kind = 'rule'` items always get promoted to L1).

### Plugin contribution

Plugins add mem_items via the `Plugin` trait at activation:

```rust
pub trait Plugin: Send + Sync {
    fn id(&self) -> &str;
    fn activate(&mut self) -> Result<(), String>;
    fn deactivate(&mut self) -> Result<(), String>;

    // Existing Chunk B method
    fn agent_providers(&self) -> Vec<Box<dyn AgentProvider>> { vec![] }

    // NEW: plugin contributes initial mem_items (typically skills) at activation.
    // Deactivation removes them automatically (PluginManager tracks ownership).
    fn mem_items(&self) -> Vec<MemItem> { vec![] }
}
```

On plugin activation, `PluginManager` calls `plugin.mem_items()`, stamps each with `source = "plugin:<id>"`, and inserts into `mem_items`. On deactivation, all mem_items with matching source are removed. This is how the `codeterm` bundled skill (and future plugin-shipped skills like a hypothetical `agent_aider`) end up in the same query surface as user-captured facts.

### Workspace-local mem_items

Items scoped to a specific workspace (`scope = 'workspace:<id>'`) only surface when the query is workspace-scoped. Example: `project-specific notes`, `this-repo's coding conventions`, `per-project Telegram IDs`. Not synced to the mesh by default.

Query pattern:

```bash
# From inside a GA session — searches global + active workspace
codeterm mem search "API timeout"

# Explicit global-only
codeterm mem search "API timeout" --scope global

# Explicit workspace-only
codeterm mem search "API timeout" --scope workspace:abc123
```

### Skill mem_item shape

Legacy-style SKILL.md with YAML frontmatter still works — we read it during the one-time migration (see Phase 3.1). After migration the content lives as a mem_item:

```rust
MemItem {
    id: "skill_home_assistant".into(),
    kind: MemKind::Skill,
    scope: "global".into(),
    title: "home-assistant".into(),
    summary: "Control HA entities, scenes, and automations".into(),
    area: Some("home".into()),
    tags_json: serde_json::json!(["home", "automation", "zigbee"]).to_string(),
    body_inline: None,
    body_blob_path: Some("blobs/sha256_a1b2c3.md".into()),  // full SKILL.md body
    importance: 3.0,
    ...
}
```

Associated scripts and reference files become sibling mem_items with `kind = 'script'` / `kind = 'note'` and a graph relation `has_script` / `has_reference`:

```bash
codeterm mem graph relate --from skill_home_assistant --rel has_script --to script_ha_api_py
codeterm mem graph relate --from skill_home_assistant --rel has_reference --to note_ha_glossary
```

An agent searching for "home automation" gets the skill back; the skill body tells it to run `codeterm mem get script_ha_api_py` to pull the script contents before executing.

### Bridge rule — one page, one CLI, no index

The bridge rule does **NOT** list available skills. It does not build a compact index. It teaches **one thing**: how to query mem.

Generated payload (same content across all providers, delivery format differs):

```markdown
## CodeTerm Mem

CodeTerm has a unified memory system containing skills, learned facts, notes,
scripts, peer info, user preferences, and past conversation excerpts. Query
it before doing any integration work, before asking the user questions you
might already have answers for, and whenever a task touches something you
don't remember clearly.

### Primary commands

- `codeterm mem search "<query>"` — full-text search across all kinds.
  Returns ranked items with id, title, and short summary.
- `codeterm mem get <id>` — print full body of one item (skill instructions,
  note, script, whatever it is).
- `codeterm mem recall "<query>"` — semantic-first search when the words
  you'd type might not match exactly (vectors if enabled, FTS5 fallback).
- `codeterm mem save --kind <kind> "<content>"` — capture something new
  you learned or decided. Use `--kind fact` for decisions, `--kind learning`
  for pending-to-validate insights, `--kind note` for short context.
- `codeterm mem wake-up` — prints the critical L0+L1 context. The GA runs
  this on init; regular tabs can run it if they need a fresh overview.

### Filters

Add `--kind skill` / `--kind fact` / `--kind script` etc to narrow search.
Add `--scope workspace` to restrict to the current workspace's items (global
items are always included by default).

### Graph queries (temporal facts and relations)

- `codeterm mem graph related --id <id>` — what's connected to this thing
- `codeterm mem graph query --type Task --where '{"status":"open"}'` — entity filter
- Add `--as-of <date>` for as-of-<date> temporal queries (stale facts auto-filtered)

### Invariants you should know

- You never need to list all skills. Search instead. The mem store is lazy.
- Every `save` is durable across sessions, providers (Claude ↔ Codex), and
  CodeTerm restarts. There is no other persistent memory — do not save to
  `~/.claude/projects/*/memory/` or similar provider-local stores.
- You cannot write to `CLAUDE.md`, `AGENTS.md`, `AGENT.md` — those are auto-
  generated by CodeTerm. Use `codeterm mem save` instead.
```

**Delivery per provider:**

| Provider | Destination | Mechanism |
|----------|-------------|-----------|
| **Claude** | `~/.claude/rules/codeterm.md` | Written as a Claude rule file — always-loaded into system prompt |
| **Codex** | Inlined into `~/.codeterm/general/AGENTS.md` | Section in the existing regenerated file |
| **Generic / local LLM** | System-prompt prepend at spawn time | `AgentProvider::deliver_prompt()` in core |

The content is **the same** for all providers — only the delivery path differs. Providers implement `AgentProvider::install_bridge_rule(&self, payload: &str) -> Result<(), String>` to write the payload where their agent will see it; `uninstall_bridge_rule(&self)` to remove it. Default impls are no-op for providers that handle the bridge some other way.

### Bridge rule regeneration triggers

The bridge rule content is **static** across sessions — it doesn't include a live skill index so it rarely needs regenerating. Triggers:

- **First run / upgrade** — generate + install on every registered provider
- **Plugin activate / deactivate** — `contributions-changed { kind: "agent_provider" }` event → re-install on the current provider set (new plugin → new provider → needs the rule written to its destination)
- **Bridge rule template change** — rare, happens only when we update the payload text in a release

**Removed compared to the old plan:** no compact skill index build, no per-skill-install regeneration. If the user adds 100 skills, the bridge rule stays the same 1-page file — agents discover those skills on demand via `codeterm mem search`, not by scanning an index.

## Layered wake-up stack (port from MemPalace `layers.py`)

The GA session-init flow runs `codeterm mem wake-up` which returns a layered text block with a token budget. Ported idea from MemPalace's L0/L1/L2/L3 model in `references/mempalace/mempalace/layers.py`, adapted for our unified mem_items store.

```
L0  identity            ~50-100 tokens    always loaded
L1  critical facts      ~500-800 tokens   always loaded
L2  area recall         ~variable         loaded when a topic comes up (NOT in wake-up)
L3  deep search         ~variable         fallback for vague queries (NOT in wake-up)
```

### L0 — Identity

What the agent IS. Sourced from a single mem_item with `kind = 'preference'` and a well-known id `preference_identity`. Contains: the name the user gave this GA (`"Rollo"`, etc), its personality traits, its role ("boss of BLACKSTAR"), its working relationships. **Always loaded** on wake-up.

If the mem_item doesn't exist yet (fresh install, no identity set), L0 returns a prompt directing the user to name the agent — `"No identity configured. Tell me what to call you and I'll save it."`

Analog to MemPalace's `~/.mempalace/identity.txt` but stored as a mem_item (same query path) instead of a separate file.

### L1 — Critical facts

Top-N most-important, most-recent mem_items. Ranked by `importance` column (default 3.0, can be bumped manually or by dreaming). Kinds included: `rule`, `preference`, `peer` — always. `fact` and `summary` — if importance ≥ 4.0. `skill`, `script`, `daily`, `learning`, `conversation`, `note` — **excluded** from L1 by default (agent will find them via search when relevant).

Grouped by `area` for readability (e.g. "## Peers" lists peer mem_items, "## Preferences" lists preference mem_items).

Cap: ~3200 characters / ~800 tokens. Implementation mirrors `Layer1.generate()` from `layers.py:76-150` but against SQL `ORDER BY importance DESC LIMIT N` instead of ChromaDB metadata fetch loop.

### L2 — Area recall (on demand)

**Not in wake-up output.** L2 is a code path the agent uses when it detects a topic area: `codeterm mem search --area <area>`. Returns top-N items scoped to that area. Triggered by the agent itself, not by wake-up.

### L3 — Deep semantic search (on demand)

**Not in wake-up output.** Full semantic search via `codeterm mem recall` — uses sqlite-vec vectors if Phase 3.5 embeddings are enabled, falls back to FTS5 BM25 otherwise.

### `codeterm mem wake-up` output format

```markdown
## L0 — Identity

I am Rollo, the General Agent on workstation. Boss of this machine.
Personality: direct, concise, no bullshit.
Relationship: main user is $USER, communicates in Russian/English mix.

## L1 — Critical context

### Peers
- ainyiva-bot (mac mini) — paired, handles iOS builds
- home-laptop (macbook air) — paired, handles macOS dev
- (1 more peer paired, 0 offline)

### Preferences
- Dark theme, HTML formatting for Telegram
- Replies: concise, skip preamble
- Build: uses ./build.sh for everything

### Rules
- Never edit CLAUDE.md / AGENTS.md (auto-generated)
- Save persistent context to mem, never to Claude auto-memory
- Ask before destructive git operations
```

Token count auto-reported as comment on the last line for debugging: `<!-- total: 672 tokens -->`.

### Wake-up budgets

| Command | Default budget | What it returns |
|---|---|---|
| `codeterm mem wake-up` | ~800 tokens | L0 + L1 |
| `codeterm mem wake-up --budget 200` | 200 tokens | L0 + compressed L1 |
| `codeterm mem wake-up --l0-only` | ~100 tokens | Just L0 |
| `codeterm mem wake-up --no-l0` | ~600 tokens | Just L1 (for reloads) |

## Learnings Capture

Port of self-improving-agent pipeline. CLI-native, no hooks required.

```bash
# Quick capture from any tab
codeterm mem learn "API timeout fixed by increasing to 30s"
codeterm mem learn --error "sqlite lock timeout on concurrent writes"
codeterm mem learn --feature "need --format json flag on skill list"

# Review
codeterm mem learn list                              # pending learnings
codeterm mem learn list --status resolved             # resolved
codeterm mem learn promote LRN-20260406-001           # → MEMORY.md

# Auto-capture (optional hook in settings.json)
# PostToolUse hook on Bash: runs error-detector, logs to .learnings/ERRORS.md
```

### Learnings → Graph integration

Recurring learnings become graph entities:

```bash
# After 3rd occurrence of same error pattern:
codeterm mem graph create --type Issue --props '{"name":"sqlite lock timeout","status":"open","recurrence":3}'
codeterm mem graph relate --from issue_xxx --rel affects --to skill_memory
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
   {"hooks": {"PreCompact": [{"type": "command", "command": "codeterm mem flush --pane $CODETERM_PANE_ID"}]}}
   ```
2. **PTY detection** (fallback): detect "conversation compressed" / compaction markers in terminal output — at this point context is already lost, but we can still save the raw PTY buffer
3. **Bridge rule teaches proactive saves** — agent periodically calls `codeterm mem save "..."` itself, not waiting for compaction

### Codex / Other agents

1. **PTY detection**: agent-specific compaction markers
2. **Bridge rule**: inject `codeterm mem save` pattern into agent instructions
3. **Session end**: always flush on pane exit (pane lifecycle hook, already exists)

### Flush: two-stage approach

**Stage 1 — Agent-side (proactive, before compaction):**
The agent itself decides what's important and calls `codeterm mem save "decided to use X because Y"`. Bridge rule teaches this. High quality, low noise.

**Stage 2 — System-side (safety net, on pane exit or compaction):**
```bash
codeterm mem flush --pane PANE_ID
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
  - Agent uses `codeterm mem search --kind skill / codeterm mem get`
  - Optional explicit export shortcut can exist later, but not as canonical delivery

Memory:
  - NOT duplicated to provider directories
  - Agent uses `codeterm mem search`, `codeterm mem fact read`, `codeterm mem graph ...`
```

### Codex

```
Bridge: installed via Codex config / prompt bridge
  - Same logical content as Claude bridge, different delivery

Skills: same broker pattern — `codeterm mem search --kind skill / codeterm mem get`
Memory: same broker pattern — `codeterm mem search/query/fact read`
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

- Know about skills via bridge rule (`codeterm mem search --kind skill`)
- Can save/search memory via CLI
- Can read/write graph via CLI
- Don't get full skill index at start — load on demand

### Worker tabs (spawned by orchestrator)

- Get task-specific context from parent
- Can use `codeterm mem learn` to capture learnings
- Report back via `codeterm agent report`
- Minimal skill knowledge (only what's needed for task)

### Canonical use case: agent queries mem on demand

This is the scenario that drives the unified mem design. Bridge rule teaches the CLI; agent searches when relevant; everything stays lazy.

```
1. User opens a workspace tab (Claude session in their project, NOT the General Agent).

2. User: "Send a message to my home assistant about the low temperature in the living room".

3. Agent has the bridge rule loaded from ~/.claude/rules/codeterm.md — it teaches
   `codeterm mem search/get/recall` but lists NO specific skills. The agent's
   session prompt stays minimal (~1KB bridge rule payload).

4. Agent thinks: "I need home automation capabilities I don't have loaded yet."
   Runs `codeterm mem search "home assistant notify" --limit 3` via Bash.

5. mem returns ranked results:
     skill_home_assistant (0.92)  — Control HA entities, scenes, automations
     fact_ha_notify_ttl (0.71)    — HA notify persistent defaults to 15s
     script_ha_api_py (0.63)      — REST wrapper for HA /api/services

6. Agent calls `codeterm mem get skill_home_assistant` → full skill body.
   The body includes reference to script_ha_api_py.
   Agent calls `codeterm mem get script_ha_api_py` → runnable Python.

7. Agent executes the flow to send the notification.

8. Agent calls `codeterm mem save --kind learning "HA notify needs ttl=60 for kitchen
    display" --tags ha,notify,ttl` so dreaming can promote it to a fact later.
```

**Key property:** the bridge rule does NOT need to know what skills exist. Adding a new skill to mem = zero change to the bridge rule, zero re-delivery, zero provider-side sync. The agent finds new content the moment it exists because `mem search` hits the live index.

### Fallback escalation

When the first search doesn't find a clear winner:

```bash
# 1. Try keyword-first (default, FTS5 BM25)
codeterm mem search "home control"

# 2. Broaden to semantic recall (sqlite-vec, Phase 3.5 — falls back to FTS5 if
#    embeddings disabled)
codeterm mem recall "what can I use to control lights"

# 3. Graph traversal — "what do we already have linked to THIS thing I know?"
codeterm mem graph related --id skill_home_assistant --rel has_script
```

The bridge rule teaches this escalation order so agents don't give up after a no-match on literal keywords.

### Invariant: project sessions never see CodeTerm data in their provider skill directories

`~/.claude/skills/` and `~/.agents/skills/` are USER territory. CodeTerm MUST NOT write skill files there. Project sessions reach mem_items exclusively through the `codeterm mem` CLI.

This is the opposite of what `src-tauri/src/service/sync_watcher.rs` does today — it mirrors `~/.codeterm/skills/` into `~/.claude/skills/` with `clean_stale` that would silently delete user-installed Claude skills. Phase 3.0 reverts that.

## Config Schema

```toml
[mem]
enabled = true                               # master switch (mem is core; disabling = agents lose persistent context)
db_path = "~/.codeterm/mem/knowledge.db"     # single SQLite file
blobs_dir = "~/.codeterm/mem/blobs"          # content-addressed large bodies
daily_retention_days = 90                    # auto-cleanup old daily notes

[mem.graph]
enabled = true                               # graph engine (lightweight, always on)
schema_path = "~/.codeterm/mem/graph-schema.yaml"

[mem.wake_up]
l0_budget_tokens = 100                       # identity cap
l1_budget_tokens = 700                       # critical facts cap
l1_include_kinds = ["rule", "preference", "peer"]  # always included
l1_threshold_importance = 4.0                # extra kinds need importance >= this

[mem.embeddings]
enabled = false                              # opt-in: requires sqlite-vec
provider = "local"                           # "local" (sqlite-vec) | "openai" | "voyage"
model = "all-MiniLM-L6-v2"                   # local embedding model
dimension = 384

[mem.dreaming]
enabled = false                              # opt-in: cron-based consolidation
schedule = "0 3 * * *"                       # cron expression
agent = "claude"                             # which agent runs dreaming
phases = ["light", "deep"]                   # which phases to run (REM optional)
max_tokens_per_run = 50000                   # approximate budget — hard-stop runaway consolidation

[mem.learnings]
auto_capture = false                         # hook-based error detection
promote_threshold = 3                        # recurrence count before auto-promote

[mem.provider_bridge]
enabled = true                               # install/update bridge rules to providers
providers = ["claude", "codex"]             # which providers to sync

[mem.mesh_sync]
enabled = false                              # opt-in replication across trusted mesh peers
mode = "manual"                              # "manual" | "scheduled" | "eventual"
replicate = ["rule", "preference", "skill", "fact"]  # mem_item kinds to replicate
exclude_scopes = ["workspace:*"]             # never replicate workspace-scoped items
max_graph_nodes = 10000
max_vector_docs = 15000
```

## Implementation Phases

### Phase 3.0 — Stop mirroring skills, install static bridge rule (PREREQUISITE)

The current `src-tauri/src/service/sync_watcher.rs` mirrors `~/.codeterm/skills/` into `~/.claude/skills/` AND `~/.agents/skills/` with `clean_stale` that would silently delete user-installed Claude skills. Phase 3.0 stops that and installs the one-page bridge rule teaching `codeterm mem *`. **No other Phase 3 work proceeds until Phase 3.0 lands.**

Much simpler than the old plan — no compact skill index, no per-skill regeneration, no lookup hierarchy. Bridge rule is a static template; it mentions the CLI and the agent finds everything else via `mem search` on demand.

- [ ] **Stop touching `~/.claude/skills/` and `~/.agents/skills/` entirely.** Remove `provider_skill_dirs()`, `sync_skills()`, `mirror_dir`, `copy_recursive`, `clean_stale`, and `sync_skills_now` from `sync_watcher.rs`. No migration backup needed — we simply stop doing the destructive thing, and user-installed entries in those dirs stay intact because we no longer touch them.
- [ ] **Add `AgentProvider::install_bridge_rule(&self, payload: &str) -> Result<(), String>`** to the trait, default no-op. Plus `uninstall_bridge_rule(&self) -> Result<(), String>` default no-op.
- [ ] **Claude impl** writes `payload` to `~/.claude/rules/codeterm.md` (atomic tmp+rename).
- [ ] **Codex impl** regenerates `~/.codeterm/general/AGENTS.md` with a new `## CodeTerm Mem` section. Reuses the existing `regenerate_agents_md` helper in `sync_watcher.rs` — pass the bridge payload as an extra section.
- [ ] **Bridge payload source** — a new `src-tauri/prompts/bridge_mem.md` template compiled into the binary via `include_str!`. One page, static content, teaches `codeterm mem search/get/save/recall/wake-up` + the escalation order + the invariants. The template does NOT enumerate skills.
- [ ] **Trigger points** — bridge rule install happens:
  - On CodeTerm startup (once per registered provider)
  - On `contributions-changed { kind: "agent_provider" }` event (new provider activated → write its rule file)
  - On `mem_items` changes — **NO**, explicitly not. The bridge content doesn't depend on what's in mem.
- [ ] **`sync_watcher::start()` signature** gains `agent_registry: Arc<AgentRegistry>` so it can iterate providers and call `install_bridge_rule`.
- [ ] **Update lib.rs call site** to pass the registry.
- [ ] **Tests**:
  - Bridge rule round-trip (install → verify file content → uninstall → verify file removed)
  - Claude provider writes to `~/.claude/rules/codeterm.md` and not elsewhere
  - Codex provider only modifies `AGENTS.md`, doesn't touch any provider dir
  - After `uninstall_bridge_rule()`, no Claude / Codex provider files remain from us

### Phase 3.1 — Mem store + graph engine + unified CLI (2 weeks)

Combines the old Phase 3.1 (memory store) and Phase 3.2 (skill broker) because they're the same system now.

- [ ] `src-tauri/src/mem/mod.rs` — MemManager facade, all writes go through here (API-mediated, not direct file writes)
- [ ] `src-tauri/src/mem/store.rs` — SQLite schema setup, `mem_items` CRUD, blob management (content-addressed), WAL mode, short transactions
- [ ] `src-tauri/src/mem/graph.rs` — entities + triples with temporal validity, entity-first traversal, as-of queries
- [ ] `src-tauri/src/mem/search.rs` — FTS5 query builder with `kind` / `scope` / `area` / `tags` filters; ranking by BM25 + importance + recency decay
- [ ] `src-tauri/src/mem/layers.rs` — L0/L1 assembly for `wake-up` command; budgets from config
- [ ] **Dispatch commands** — `MemSearch`, `MemGet`, `MemSave`, `MemRecall`, `MemWakeUp`, `MemLearn`, `MemFlush`, `MemGraph*` through unified dispatch (same pattern as `ListAgentProviders` in Chunk B)
- [ ] **CLI subcommands** — `codeterm mem search/get/save/recall/wake-up/learn/flush/graph *` all wrap dispatch
- [ ] **REST endpoints** — `POST /api/mem/search`, `GET /api/mem/items/:id`, `POST /api/mem/items`, etc. Thin adapters over dispatch.
- [ ] **Socket methods** — `mem.search`, `mem.get`, `mem.save`, etc.
- [ ] **Tauri commands** — `mem_search`, `mem_get`, `mem_save`, etc. for frontend use (e.g. a Mem browser UI panel later).
- [ ] **Plugin `Plugin::mem_items()` trait method** — plugins contribute initial mem_items at activation. `PluginManager` stamps `source = "plugin:<id>"` and inserts; removes on deactivation.
- [ ] **One-time migration** — on first run after upgrade, scan `~/.codeterm/skills/<name>/SKILL.md` files, parse frontmatter (name, description, tags), insert as `mem_item { kind: 'skill' }` with body stored in `blobs/`. Set marker file `~/.codeterm/mem/.legacy-skill-migration-done` so we don't re-run.
- [ ] **Importer**: `codeterm mem import --from <path>` bulk-imports a directory of skill folders or a JSON dump.
- [ ] **Legacy ontology importer**: `codeterm mem graph import-ontology --from graph.jsonl`
- [ ] **Tests**: unit tests for graph operations, SQLite round-trips, schema validation, FTS5 queries with filters, migration of legacy skills directory, plugin mem_items contribution lifecycle (activate → insert → deactivate → remove), temporal graph queries with as-of dates

### Phase 3.2 — Bundle starter mem_items + bridge rule finalization (3 days)

After Phase 3.1 the store exists and the CLI works. Phase 3.2 seeds the starter content and finalizes the bridge rule payload.

- [ ] **Bundle starter skills as mem_items** — `codeterm`, `ontology`, `skill-creator`, `skill-evaluator`, `self-improving-agent` as pre-seeded `kind = 'skill'` items shipped with the binary (embedded via `include_str!` or a bundled SQL dump loaded at first run).
- [ ] **Bundle starter rules** — `never-edit-autogen-files`, `save-to-mem-not-claude-memory`, `report-back-after-delegation`, etc. as `kind = 'rule'` items (the always-on L1 content).
- [ ] **Bundle starter preferences template** — empty `kind = 'preference'` items the GA fills in via the bootstrap flow.
- [ ] **Bridge rule payload** — finalize the `src-tauri/prompts/bridge_mem.md` template with concrete examples, escalation order, invariants. Round-trip tested via prompt-evaluator.

### Phase 3.3 — Learnings capture + compaction flush (1 week)

- [ ] `src-tauri/src/mem/learnings.rs` — capture, categorize, promote
- [ ] CLI: `codeterm mem learn`, `codeterm mem learn list/promote`
- [ ] `codeterm mem flush --pane ID` — pre-compaction save (grabs PTY buffer, stores as raw conversation mem_item)
- [ ] Claude hook: PreCompact → `codeterm mem flush`
- [ ] PTY detection: compaction markers for Claude/Codex (backup path when PreCompact hook doesn't fire)
- [ ] Pane lifecycle: auto-flush on agent exit

### Phase 3.4 — Dreaming consolidation (1 week, opt-in)

- [ ] Cron job creation on `config set mem.dreaming.enabled true`
- [ ] Dreaming agent task prompt (Light + Deep phases)
- [ ] Graph-aware consolidation (group related mem_items before promoting)
- [ ] Short-term recall tracking
- [ ] Structured writes first: mem_items + relations + optional vectors
- [ ] Curated summary mem_items (kind = `summary`) get high `importance` to float up into L1
- [ ] Mesh-aware replication policy for curated outputs

### Phase 3.5 — Embeddings + semantic recall (optional, 1 week)

- [ ] Integrate `sqlite-vec` as a loaded SQLite extension (dynamic via `rusqlite::Connection::load_extension`)
- [ ] Create `mem_vectors` virtual table with embedding column (dimension from config)
- [ ] Hybrid search: vector similarity + BM25 + temporal decay
- [ ] MMR re-ranking for diversity
- [ ] Embedding providers: local (`fastembed-rs` with all-MiniLM-L6-v2) or API (OpenAI, Voyage)
- [ ] Index all mem_items (backfill existing + on-write hook for new)
- [ ] `codeterm mem recall "query"` enhanced with semantic search when enabled; falls back to FTS5 otherwise

## Migration Path for Existing Skills

User's 44 skills currently in `~/Desktop/clawd-skills-scripts/skills/` and the bundled CodeTerm skills in `~/.codeterm/skills/`:

```bash
# Phase 3.1 migration runs automatically on first post-upgrade startup.
# After that:

# Manual bulk import of external skill library:
codeterm mem import --from ~/Desktop/clawd-skills-scripts/skills/ --kind skill

# Or per-skill:
codeterm mem save --kind skill --title home-assistant \
  --body "$(cat ~/Desktop/clawd-skills-scripts/skills/home-assistant/SKILL.md)" \
  --tags home,automation,zigbee
```

This copies the whole skill folder to `~/.codeterm/skills/{name}/`, preserving local `scripts/`
and `references/`, then registers it in the broker/graph.

Skills that become plugins (home-assistant, telegram, etc.) will later be migrated:
```bash
# Plugin subsumes the skill — plugin activation injects its mem_items via
# Plugin::mem_items() trait method; no separate install step needed:
codeterm plugin install ha-bridge

# Old manually-imported standalone skill can be removed if duplicated:
codeterm mem delete skill_home_assistant
```

## Hardening (from architecture review)

### Concurrency: all writes through MemoryManager

Multiple tabs call `codeterm mem graph create` simultaneously. Direct file writes or ad hoc
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
codeterm mem graph maintain   # ANALYZE/VACUUM/rebuild derived indexes
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
Use `codeterm mem get <name>` for full instructions.
```

~660 tokens for 44 skills. Regenerated on skill install/uninstall. Agents see what's available without running `skill list`.

### Search quality without embeddings

Keyword grep is useless for semantic recall. Default search must be BM25 minimum.

**Solution:** SQLite `FTS5` is the default search layer. Tags on save:
```bash
codeterm mem save --tags "api,timeout,fix" "increased deadline to 30s"
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
codeterm mem diff --since "10m ago"    # what changed recently
```

### Graph query ergonomics

JSON-in-shell-quotes is error-prone.

**Solution:** Key-value sugar for common queries:
```bash
codeterm mem graph list --type Task --status open     # sugar
codeterm mem graph query --type Task --where '{"status":"open"}'  # full power
codeterm mem graph stats                               # entity counts by type
```

### Cold start

First time, everything empty. Agent tries commands, gets nothing, stops trying.

**Solution:**
- `codeterm mem init` — creates dirs, seeds with bootstrap entities (user, machine), runs the legacy-skill migration, loads bundled starter mem_items
- Empty `codeterm mem search --kind skill` prints: "No skills in mem yet. Import: `codeterm mem import --from <dir> --kind skill` or `codeterm mem save --kind skill ...`"
- Empty `codeterm mem search` prints: "No mem_items yet. Save with: `codeterm mem save --kind fact \"...\"`"
- First CodeTerm start auto-runs `mem init`

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

1. **`mem` is CORE, not a plugin.** Graph + store are too fundamental — plugins depend on them, can't be one themselves.
2. **Unified `mem_items` table.** Skills + facts + notes + conversations + scripts + peers + preferences + daily + learnings + rules are ONE table discriminated by a `kind` column. No separate skill broker, no separate memory store, no dual CLI.
3. **SQLite is the single source of truth.** One `knowledge.db` file. Backup = copy. Mesh sync = targeted replication of selected rows. `MEMORY.md` (if any) is a human-readable derived view, not the database.
4. **`mem` is the single agent-facing CLI.** `codeterm mem search/get/save/recall/wake-up/learn/flush/graph *`. There is no `codeterm skill *`, no `codeterm memory *` — just `codeterm mem *`.
5. **Bridge rule is a static one-page payload, no compact skill index.** Teaches the CLI, not the catalog. Agents discover content on demand via `mem search`. Bridge content doesn't change when the user adds skills.
6. **Bridge rule delivery is owned by the provider.** `AgentProvider::install_bridge_rule(&str)` — Claude writes `~/.claude/rules/codeterm.md`, Codex regenerates an AGENTS.md section, generic injects via `deliver_prompt()` at spawn. Default no-op for providers that handle it differently.
7. **Provider skill directories belong to the user.** `~/.claude/skills/` and `~/.agents/skills/` are NEVER touched by CodeTerm. No mirror, no backup pass, no cleanup. User-installed Claude skills in those dirs stay untouched, parallel to our mem_items.
8. **Dreaming writes alongside verbatim, never replacing raw.** Raw conversation / PTY flush is preserved as `kind = 'conversation'`. Dreaming produces `kind = 'summary'` items that live alongside, not replacing the raw source. Agents see both; `mem wake-up` prefers summaries for L1 brevity.
9. **All writes through `MemManager`** (API-mediated). CLI never writes DB files directly — it calls `dispatch::Command::Mem*` which goes through the same transaction queue as REST/Socket/Tauri. Prevents races.
10. **Pre-compaction flush for GA + PreCompact hook for Claude tabs.** Agent-side proactive saves via bridge rule instructions (`codeterm mem save "decision X because Y"`).
11. **Mem is local-first.** Mesh sync is opt-in, selective (whitelist of kinds), never includes secrets/raw conversation, never workspace-scoped items unless explicitly opted in.
12. **Schema validation on every write.** Not just a check command — runs in the write path for entities + relations. Mem_items have kind-based validation too (e.g. `kind = 'skill'` requires a `title`).
13. **Dreaming is opt-in.** Uses existing cron system. Disabled by default (cost concern).
14. **Temporal validity windows on graph relations.** `valid_from` / `valid_to` on `relations` table (port from MemPalace). `as-of` queries filter automatically. Invalidated facts don't pollute recall.
15. **Layered wake-up stack (L0-L3).** Ported from MemPalace `layers.py`: L0 identity (~100 tok) + L1 critical facts (~700 tok) in wake-up, L2 area recall + L3 deep semantic search are on-demand only. Explicit token budgets in config.
16. **No Python, no ChromaDB.** Fully Rust + SQLite. `sqlite-vec` extension for vectors (Phase 3.5 opt-in). Port MemPalace's ideas from their code (`references/mempalace/`), not their implementation — their choices (Python runtime, ChromaDB second store, AAAK dialect) are incompatible with our Rust-native single-file-backup architecture.
17. **`codeterm mem wake-up` is the GA init injection source.** Backend sends `init` to freshly spawned GA (existing flow from commit `a13fa35`), GA runs `codeterm mem wake-up` as its first action, gets L0+L1 context, acks ready. No separate identity file — identity lives as a mem_item.
18. **Plugin `mem_items()` trait method.** Plugins contribute initial mem_items at activation; PluginManager stamps `source = "plugin:<id>"` and removes on deactivation. How bundled skills (`codeterm` skill) get into the store without special-case code.

## MemPalace port notes

Specific architectural decisions ported from reading MemPalace's Python code at `references/mempalace/mempalace/`. Each is adapted to Rust + SQLite; we take the _idea_, not the implementation.

### From `searcher.py`

**Port:** metadata filter (`wing + room`) combined with similarity ranking. That's just a SQL `WHERE` clause on `mem_items` + ORDER BY distance (or BM25).

```sql
-- MemPalace: col.query(query_texts=[q], where={"$and":[{"wing":w},{"room":r}]}, n_results=5)
-- CodeTerm equivalent:
SELECT m.id, m.title, m.summary, bm25(mem_fts) as rank
FROM mem_items m
JOIN mem_fts ON mem_fts.mem_id = m.id
WHERE m.scope = :scope
  AND m.kind = :kind
  AND m.area = :area
  AND mem_fts MATCH :query
ORDER BY rank
LIMIT 5;
```

**Reject:** ChromaDB dependency. Their "palace boost" is literally the `WHERE` clause — we get the same with SQL without a second storage engine.

### From `knowledge_graph.py`

**Port:** temporal triples model. Tables `entities(id, name, type, properties)` + `triples(subject, predicate, object, valid_from, valid_to, confidence, source_closet, source_file)`. Indexes on subject/object/predicate/valid range. `query_entity(name, as_of=date)` entity-first traversal with time filter.

**Port:** idempotent `add_triple` with computed ID from `hashlib.sha256(subject+predicate+object)[:16]` — prevents duplicate triples. We do the same in `mem::graph::Relation::id()`.

**Port:** entity normalization via lowercased + space-replaced ID (`_entity_id(name)`). We do the same.

**Adapt:** they store entities + triples in a SEPARATE SQLite file (`~/.mempalace/knowledge_graph.sqlite3`) from the main ChromaDB. We put everything in `knowledge.db` — one file, one transaction scope.

### From `layers.py`

**Port:** L0/L1/L2/L3 model with explicit token budgets. L0 always-loaded identity, L1 always-loaded critical facts, L2+L3 on-demand.

**Port:** L1 scoring by `importance` column, top-N, grouped by area for readability. Cap on total character count (they use 3200 chars ≈ 800 tokens).

**Adapt:** their L0 reads a plain-text file `~/.mempalace/identity.txt`. Ours reads a mem_item with id `preference_identity` — same query path as everything else, no special file.

**Adapt:** their L1 iterates all drawers via ChromaDB `col.get()` in batches of 500, then sorts in Python. Ours does `SELECT ... ORDER BY importance DESC LIMIT N` in a single SQL query. Faster and cleaner.

### From `miner.py`

**Port:** file walk with `SKIP_DIRS` (`node_modules`, `.git`, `target`, `__pycache__`, ...) and `READABLE_EXTENSIONS` list. We need the same for `codeterm mem import --from <dir>` bulk importer.

**Port:** chunking at ~800 chars with ~100 char overlap for conversational content. Their `CHUNK_SIZE = 800`, `CHUNK_OVERLAP = 100`, `MIN_CHUNK_SIZE = 50`. Copy these defaults as constants.

**Port:** `GitignoreMatcher` for respecting `.gitignore` during import. We already have one (via `ignore` crate likely) but their minimal implementation is useful reference.

**Reject:** their "auto-detect wing from `mempalace.yaml`" approach. We use CodeTerm's existing workspace model — the workspace IS the wing.

### From `general_extractor.py`

**Port:** classification of text into memory types (`decision`, `preference`, `milestone`, `problem`, `emotional`). Maps onto our `kind` discriminator — we use slightly different names (`fact`, `preference`, `rule`, `learning`) but the classification is similar.

**Port:** "classify at ingest time, not at query time" principle. Each mem_item has a `kind` stored; queries filter by kind rather than re-classifying on the fly.

### From `convo_miner.py`

**Port:** Q+A exchange pairing for conversation chunking. When flushing a PTY session (`codeterm mem flush`), chunk on user-prompt / agent-reply pairs instead of fixed char windows. Preserves conversational structure.

**Port:** room auto-detection from content keywords. We don't need rooms per se, but the idea of tag inference from body content is useful for the importer.

### From `mcp_server.py`

**Port (eventually):** MCP server exposing mem tools to agents. Our `codeterm mem *` CLI is the primary interface (per plugin-architecture decision), but an MCP wrapper is a nice-to-have for MCP-native ecosystems (Cursor, Windsurf, etc). Phase 3.5+ or a follow-up chunk.

**Reject:** their "19 tools" surface. We'd expose maybe 5 — `mem_search`, `mem_get`, `mem_save`, `mem_recall`, `mem_wake_up`. Keep the tool count small so agents don't drown in schemas.

### What we explicitly are NOT porting

| MemPalace feature | Why skip |
|---|---|
| **AAAK dialect** (`dialect.py`, 1075 LOC — biggest file in the repo) | Maintainers' own correction says it regresses at small scale (73 vs 66 tokens: worse). Custom compressed-instruction format is maintenance burden with no proven win. |
| **ChromaDB** for vectors | Second storage engine, Python dependency, non-Rust. `sqlite-vec` gives us the same with one file, Rust native. |
| **`entity_detector.py`** auto-detection | 853 LOC of NLP heuristics to find people / projects in text. For CodeTerm our entities come from explicit graph operations (`codeterm mem graph create --type Task ...`), not auto-detection. If we want this later it's a separate agent-side skill, not core mem. |
| **`room_detector_local.py`** (70+ folder → room patterns) | We don't have rooms; we use tags + areas freely. Manual tagging is fine for a project-scoped tool. |
| **`spellcheck.py`** | Proper-noun-preserving spellcheck is a niche need we don't have. |
| **Palace vocabulary** (wings / rooms / halls / drawers / closets / tunnels) | Evocative metaphor but adds cognitive load. Our model is flat: `mem_items` + `area` tag + graph relations. UX decision, not architectural. |
| **`onboarding.py`** guided first-run | We already have the bootstrap flow via `~/.codeterm/general/bootstrap.md` + GA init kick. Don't need a second onboarding path. |
