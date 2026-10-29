# Domios Memory System

Persistent memory survives restarts, provider switches and upgrades.

## Overview

The selected memory backend holds text/vector indexes and temporal people/project relations. SQLite is the default (`~/.domios/mem/knowledge.db`, with large bodies beside it); Postgres with pgvector is selectable through `memory.backend`. Pins lead wake-up selection, followed by importance, use, workspace and recency (default budget 300 tokens). Folder skills are indexed too. Hidden workers enrich memory; the Librarian syncs source files. Dreaming starts at 6 AM with 0..30 minutes of jitter and runs light, deep, normalize, calibration and REM cleanup; merges archive rather than delete. Hooks track sessions without exporting scrollback.

Save durable facts immediately when asked to remember. Keep session/task instructions in conversation or the plan unless the user says they are permanent.

## Mem vs past-session search

Separate stores:

| Surface | Stored where | Asks | Used for |
|---|---|---|---|
| **mem** | selected SQLite or Postgres backend with text and vector indexes | "what did the agent learn / decide / save?" | Curated facts, learnings, skills, conversations the agent chose to keep |
| **sessions** (provider-owned files) | Claude JSONLs / Codex rollouts / OpenCode's sqlite store | "what did we talk about that one time?" | Raw conversation history; on-demand search via `domios sessions search`, atomic resume via `domios sessions resume` |

These stores never share storage. Session API: `domios docs API-STATE`.

## Backend selection and migration

Set `[memory] backend = "sqlite"` (default) or `"postgres"` in the instance configuration. A Postgres-enabled server reads its connection URL exclusively from the active secret store key `domios.mem:postgres_url`. Missing or empty credentials, unavailable secrets and a build without Postgres support refuse startup. The selected backend being empty while the other configured store has items also refuses startup and names the migration command.

Stop the daemon before switching. The local CLI pins its resolved instance data directory and delegates to the installed server, which holds the same instance ownership locks throughout the copy and refuses a running daemon:

```bash
domios mem backend migrate --to postgres
domios mem backend migrate --to sqlite
```

The source is always the backend opposite `--to`; this works whether configuration has already changed or still names the source. The command copies all 21 memory tables in bounded pages, including history, original authors, graph data, vectors and bodies. Source opens are read-only: no schema changes, cleanup or seeding. A source with an outdated schema must first be opened once with its own Domios. The source is retained after success or failure. The command never starts or stops a daemon and never changes configuration.

A target containing data is refused unless `--into-nonempty` explicitly authorizes import. Conflicting row identities still fail; the command does not merge or replace unrelated target records. A populated target with a different embedding stamp is refused before import. Copy counts are verified for every table. When the target embedding stamp (or configured embedder for a new target) differs from the source, the command builds and publishes a shadow index on the target. The configured remote embedder requires its existing consent and credentials. Final counts verify the copied tables and the rebuilt vector projections separately, and the summary reports all counts. Embedding failure leaves the source intact and reports migration failure.

Only after a verified success, set `memory.backend` to the destination and start Domios. An interrupted or failed migration may leave a partial target; retain the source, inspect the failure and use a fresh target before trying again. Offline migration needs the URL available through the configured secret store; a plugin-only vault unavailable outside daemon composition must be made available before migration can run. The installed CLI uses an owner-only resource hint written during installation. A missing, stale or unverifiable hint refuses migration and asks you to reinstall the CLI; it never searches PATH or guesses a server location.

## Storage layer

### Item fields

- `kind` — `fact` | `learning` | `note` | `conversation` | `skill` | `rule` | `preference` | `peer` | `daily` | `summary`
- `scope` — `global` | `workspace:<id>`; any other value is refused
- `area` — project/topic tag (`auth`, `billing`, `deploy`, `home`, `_dreams`)
- `importance` — 1-5; the main wake-up ranking term
- pinned (`mem pin <id>` / `mem unpin <id>`) — pinned items always load in wake-up, subject to the budget

Memory items and history carry an author: owner, neighbor, or system. Neighbor authors carry the registered friend's account UID and friendship ID, resolved by the daemon from authenticated friendship data. Request fields cannot choose an author. Replacement re-signs the item while history retains the prior author and body.

A concierge may save only through `domios mem neighbor-save --kind fact --area <granted-area> --title "<title>" --from-pad <body.md>` (also kind `note`, optional `--summary` and repeated `--tag`). The live `memory_write_areas` grant defaults to empty. The daemon forces global scope and unpinned content, clamps importance to 1–5, scans all content fields for credentials, and enforces byte caps of 200 title / 16384 body / 32752 total. Matching kind/title replacement requires a granted area and an unpinned target; conflict refuses without exposing the item. Revocation stops the next save and retains past authorship. These saves never schedule extraction; all other concierge memory mutations remain denied.

`mem save` uses kind plus title as its identity. An unchanged normalized body increments `recurrence_count` and schedules no extraction. A changed body updates the existing row in place and adds the prior body to history. `--replace` also updates in place and preserves the row's uid and creation time.

Editing bundled memory preserves your version; pin changes are audited. Graph nodes are people and projects only, and relations are temporal, so point-in-time queries work.

## Search & retrieval

Three layers:

| Layer | What | When | Size |
|---|---|---|---|
| **L0 — Identity** | `~/.domios/general/identity.md` | Injected into every GA init | ~100 tok |
| **L1 — Wake-up** | Pinned items + ranked by score (see below) | `domios mem wake-up` (GA runs on init) | ~300 tok (config: `[memory] l1_budget_tokens`) |
| **L2 — Targeted** | `search --area X --kind Y` | On-demand when topic comes up | variable |
| **L3 — Hybrid** | text and semantic recall | `domios mem search` / `recall` | variable |

Filters narrow hybrid search; a connected Supporter automatically re-ranks results.

### Embedder

All of memory uses exactly one embedder: items, graph entities and the documentation vectors. The default is the local on-device model (`local`, `sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2`, 384 dimensions). The selection lives in `[memory.embedder]` (`domios docs CONFIG`).

```bash
domios mem embedder list                     # providers, models, why a remote one is unavailable
domios mem embedder status                   # provider, model, re-embed progress, vector-pending count and live queue
domios mem embedder use local                # switch; prints how many items it re-embeds
domios mem embedder use openai --model text-embedding-3-small --dimension 512
```

- **Switching.** The index is stamped with provider, model and dimension. A switch builds a stamped shadow (items, entities, Blueprint solutions, documentation) in the background while recall stays semantic on the published model, then swaps atomically; content changed during the build is re-embedded first. Items the new provider refuses count as failed and stay vector pending; only an outage stops the build. Status (CLI, Blueprint `recall` state, Settings → General → Memory embedder) shows progress, failures and the cause. Choosing the published model cancels a build; another target is refused until it ends. Vectors from different models never mix.
- **Retry or cancel.** Config keeps the requested target; status shows the published model separately. Re-run `use` with the same selection, or restart, to resume checkpointed work; select the published model to discard the shadow. Both local models stay resident during a switch.
- **Outages.** When the embedder is unavailable, saves still succeed and keyword search finds them; the item stays vector pending until the provider answers.
- **Recall priority.** Queries go ahead of background embedding; one background request runs per eight consecutive queries so builds keep moving. Semantic retrieval gets 500 ms, then search returns keyword results with `degraded: "semantic pending"`. Status shows the live queue (queued, in flight, failed, availability) separately from vector-pending items.
- **Remote embedders send memory text to a third party.** A remote provider activates only with a stored API key AND `memory.embedder.remote_opt_in = true`; credential-flagged text is never sent. The key is read at activation and daemon start, never from config, argv or the environment. Sources (the file wins):
  1. `<data_dir>/secrets/<provider>-embedder.key` in the selected instance's data directory, readable only by its owner (0600 on Unix, a current-user ACL on Windows);
  2. the Domios secret store entry `<provider>-embedder`.

  Use `domios mem embedder key set <provider>` first: it reads piped stdin only, writes atomically with owner-only permissions, prints the path (never the key), follows the selected instance and refuses terminal input, argv keys and `--mesh`. Examples: `pbpaste | domios mem embedder key set openai` (macOS), `Get-Clipboard -Raw | domios mem embedder key set openai` (Windows), or pipe a password manager's stdout (Linux).

  Manual fallbacks (selected instance's secrets dir):
  - **Unix:** `umask 077; mkdir -p ~/.domios/secrets; cat > ~/.domios/secrets/openai-embedder.key && chmod 600 ~/.domios/secrets/openai-embedder.key` (paste, then Ctrl-D).
  - **Windows PowerShell:** create an empty `<data_dir>/secrets/openai-embedder.key` as `$file`, set `$owner = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name`, run `icacls $file /inheritance:r /grant:r "${owner}:(F)"`, then write the key as UTF-8 without a BOM.
  - **Secret store:** `domios mem secret save --name openai-embedder` (key on stdin).

  Asked to set it up, an agent pipes the supplied value into `domios mem embedder key set <provider>` and runs `domios mem embedder use <provider>`; the key never goes into argv, output, logs or mem.

### Supporter Agent memory re-rank

A second stage over `mem search` that runs automatically when an adapter is connected. Stage 1 remains hybrid recall, widened to a candidate pool; the adapter grades candidates and sorts them by relevance. Without a connected adapter, search behaves as before. There are no feature switches. `domios mem search --supporter on|off` is a per-request diagnostic override; without it, the connection decides. The legacy `--rerank` is exclusive with `--supporter on` (the request is refused as invalid).

- **Hits.** An applied search adds a `relevance` fraction (0..1) to each hit, shown as `rel 92%` next to the score in the human render.
- **JSON.** CLI `--json` hits contain only an item `id` (or a doc pointer), `source`, optional `via`, baseline `score`, and Supporter `relevance`; titles, bodies, summaries and tags are omitted. The Supporter envelope retains its status, fallback reason and low-confidence fields. Use `domios mem get <id> --json` for a full memory record. The HTTP search response keeps item details for the Memory UI.
- **Envelope.** The `supporter` object reports `applied` (with `adapter`, `low_confidence`, `max_relevance`), `fallback` (an adapter failure, deadline, missing adapter or exhausted budget; plain recall hits are returned with a `reason`, including `budget`), or `skipped` (`reason: legacy_rerank`). The human render prints one line for fallback/skipped with the reason. Without the key the output is exactly the plain recall render.
- **Graph source.** `[supporter.memory_rerank.sources]` sets the pool: `recall` hits (default 30) plus up to `graph` extra candidates (default 3; `0` turns the graph source off). Graph candidates are live mem items linked by an active relation to an entity whose id contains a query keyword, ranked by how many matched entities they link to, then importance and recency; they are found by SQL alone, never by a model or embedding. Items already in the recall pool are not added twice. With `graph > 0` an applied search tags every hit `via: "recall"|"graph"` and the envelope adds `sources: {recall, graph}` (pool counts). With both `graph = 0` and `docs = 0`, applied output carries neither key; every fallback also carries neither and matches the plain recall contract.
- **Docs source.** `[supporter.memory_rerank.sources].docs` adds 0..10 embedded doc sections (default 3), including CLI command headings. Keyword selection is default; `supporter.memory_rerank.docs_hybrid = true` adds semantic matching when vectors are ready, falling back to keyword order on missing vectors/errors. Applied hits contain `source: "doc"`, `doc`, `section`, `anchor`, `pointer`, `relevance`, `via: "docs"`; pointers run `domios docs <NAME>` or `domios <command> --help`. The envelope counts docs.
- **Sessions source.** `[supporter.memory_rerank.sources].sessions` adds up to five recent matching snippets (default 0), through provider session search under the rerank deadline; errors, timeouts or saturation skip it. The adapter sees title, provider, date and a capped excerpt. Applied hits contain `source: "session"`, `session_id`, `provider`, `title`, `excerpt`, `pointer`, `relevance`, `via: "sessions"`, pointing to `domios sessions get <id>`. Docs and sessions never record mem access and appear only after successful re-ranking, never in off/fallback results.
- **Access.** Pool candidates, recall or graph, record no access while they are graded. Only the hits the search returns count as accessed, once each.
- **Low confidence.** `low_confidence: true` means even the best hit scored below `low_confidence_threshold`: nothing relevant was found. The human render says so in one line; treat the hits as weak leads, not answers.
- **Tokens.** Adapters declare `required`, `optional` or `none` (`"decisionModel": true` means optional). Only required adapters need a usable stored token. Use `pbpaste | domios supporter token set --adapter <adapter>`; terminal input is refused. Tokens never enter config or argv. `token status` shows presence, path and `mode_ok`, never the value. Storage is `<data dir>/supporter/<adapter>.json`, owner-only: Unix 0600 in a 0700 directory; Windows restricts broad access while preserving explicit narrow runtime grants. Each instance keeps its own. Token commands use local bearer HTTP and refuse `--mesh`; none-mode adapters ignore writes.

Decision adapters target deterministic models such as [TypeSafe Jev](https://typesafe.ai). Fine-tunes and general LLMs may differ in accuracy and latency. Calibrated choice/noul probabilities describe decisions; sampled-text logprobs describe generated tokens and are not equivalent.

The adapter, model and token are managed in **Settings > Agents > Supporter Agent** or with `domios supporter`. Keys and bounds: `domios docs RUNTIME-CONFIG`; routes: `domios docs API-MEMORY`.

### Supporter Agent

The Supporter re-ranks memory, observes tab lifecycle events and supports bounded General Agent recovery. Connecting enables it automatically; disconnected behavior is unchanged.

- **Connections.** Discover with `domios supporter models`, save any required token, then `domios supporter use <adapter> [--model <id>]`. Settings can keep several connections with remembered models and Make active actions. `domios supporter disconnect <adapter>` removes its connection and token; removing active stops the Supporter without selecting another. `domios supporter status` shows activity, verdicts and budget.
- **Budget.** `supporter.calls_per_hour` is unlimited when absent or 0; positive values up to 100000 cap physical HTTP attempts. Local decisions cost no request. Batches count once, sequential calls separately. At the cap, search returns baseline hits with reason `budget`, observation skips, and status shows `budget_reached` plus the next free time. Unlimited status has `calls_per_hour: null`. Adapter failures also return baseline results; the default stage deadline is 3000 ms.
- **Sent data.** Memory queries and candidate titles/bodies (default cap 2000 characters), plus configured docs/session excerpts; observations send recent screen/chat text and runtime context. Jev receives this at TypeSafe; local adapters use their configured server. Tokens remain local.
- **Prompt answers.** On agent-, Team-, worker- and cron-owned tabs, a calibrated choice >=0.80 may answer folder-trust, tool-permission or update prompts through a provider-declared option. Failed, capped, `other` or low-confidence answers use the structural responder; unresolved cases notify the owner. Direct owner tabs retain prompts. Authentication, login, unavailable-model and quota prompts are never auto-answered. See `domios docs TAB-LIFECYCLE`.
- **Recovery.** At most two General Agent automatic restarts per episode, then a failure notification until owner action. A crash resumes only a verified session whose provider is absent from the tab's process tree. Startup failure and error loops each allow one attempt within the overall cap. Model-authorized recovery stays notify-only pending qualification. See `domios docs AGENT-CONTROL`.
- **Models and spend.** Jev remains the calibrated re-rank model, billed to TypeSafe. `domios model-provider` configures OpenCode local models; their optional decision adapter has no calibrated confidence. Estimates use response usage and declared prices; unpriced adapters have none. Credit exhaustion sends one failure alert and uses deterministic results until a later probe succeeds.
- **Activity.** Mission Control shows recent verdicts. The bounded `<data dir>/supporter/journal.jsonl` records actions, budget transitions, answer distributions, model identities and criteria hashes, never raw inputs or tokens. `other` triggers no action or hint. `domios mem search --supporter off` skips re-rank once. Clearing a required token keeps the saved connection but makes it unusable.

The Guide opens from a standalone `@ds-guide supporter-agent` paragraph. Jev: https://docs.typesafe.ai.

### Wake-up selection

Wake-up includes global and selected-workspace memory; pins take precedence within the budget, overflow warns you to unpin or raise it, and `--explain` shows selection scores. `domios mem wake-up` inherits `CODETERM_WORKSPACE_ID` from the calling tab; `--workspace <ws>` explicitly overrides it, and without either only `scope = global` items participate.

## Graph layer

**Anti-star pattern:** only people and projects are entities. Search technologies through body FTS; no tech nodes or popularity hubs.

**Canonical names:** dreaming worker merges name variants (Tima/Timofey → `person_tima`).

**Temporal validity:** relations have `valid_from`/`valid_to`. Dreaming's deep phase expires stale relations rather than deleting them.

**API:**
- `domios mem graph create --id <id> --entity-type Person --scope global` (idempotent)
- `domios mem graph relate --from <id> --rel mentions --to person_tima --reason "dreamer-bridge"`
- `domios mem graph related --id <id>` — outgoing relations

### Self-repair primitives

The Dreamer cleans historical pollution (duplicate relations, near-duplicate entities, orphan vectors) via typed primitives only — no raw SQL surface for the LLM. Every destructive op writes a full payload to `graph_purge_log` for reversibility.

| CLI | What it does | Audit-logged |
|---|---|---|
| `mem graph find-duplicate-relations --group-by …` | Read-only: list groups by whitelisted columns | no |
| `mem graph delete-relations --rowids 1,2,3` | Delete explicit rowids | yes (payload preserved) |
| `mem graph merge-relations --keep R --merge R1,R2` | Fold rows into `keep`, max(confidence), merge properties (canonical wins) | yes |
| `mem graph find-duplicate-entities --strategy exact-name\|alias-chain\|vector --threshold 0.85 --entity-type Tech` | Surface candidates. Vector strategy uses cosine-sim ≥ threshold | no |
| `mem graph alias --alias A --canonical C` | Merge entity A into C: rewire all relations, fold A's properties into C, delete A + its mem_vectors | yes (since #22) |
| `mem graph augment-entity --id E --patch '{"city":"Riga"}'` | PATCH entity properties + re-embed | yes (count=0) |
| `mem graph delete-entity --id E [--force] [--purge-history]` | Direct entity delete with safety net (errors if active relations and no `--force`). `--purge-history` also drops invalidated relations | yes |
| `mem graph repair-orphan-vectors [--dry-run]` | Sweep `mem_vectors` rows whose mem_id has no matching entity/mem_item | yes |
| `mem graph prune-thin-vectors [--dry-run]` | Delete vectors whose mem_item has an empty/title-only body (below the embed threshold) — the bodyless embeddings. Never touches a real-body vector or the item itself | yes |
| `mem graph collapse-synonym-edges [--dry-run]` | One-pass: per node-pair keep ONE membership predicate (about>applies_to>belongs_to>part_of>documented_in), invalidate the rest. Distinct predicates (uses, runs_on, …) untouched. Deflates hubs without hundreds of agent round-trips | yes |
| `mem graph prune-dream-hub-edges [--dry-run]` | One-pass: invalidate every dream-diary (kind=daily) → entity backlink via a noise predicate (about/applies_to/works_on/analyzes/…). Keeps the dreams anchor. Stops diaries inflating project/topic hubs | yes |
| `mem graph repair-orphan-blobs [--dry-run]` | CAS-aware sweep of `sha256_*.txt` blob files no `mem_item.body_blob_path` references | yes |

Use the bounded graph commands and their dry runs; do not issue raw SQL.

### Hub report

`mem graph hubs [--threshold 20] [--limit 5] [--sample 0]` is read-only and local. It returns at most `limit` nodes whose active degree is at or above `threshold`, highest degree first, as `{threshold, limit, sample, total_hubs, hubs, excluded, existing_anchors}`. `total_hubs` counts every hub before the limit; `excluded` lists the dreams anchor `project_dreams` (`{id, degree, reason}`), which is never a hub to decompose. Each hub is `{id, degree, rel_dist, popular_source, sub_anchors, outbound_only, unclustered, clusters, neighbors}`:

- `clusters` groups the hub's inbound neighbours (the edges `introduce-topic` can re-route) by shared area, then title keyword, then kind. A group must split the remaining neighbours and must not just repeat the hub's own name; existing `topic_*`/`role_*` neighbours count as `sub_anchors` instead. Each cluster is `{suggested_topic, entity_type, existing, basis, key, members}`: `suggested_topic` reuses an existing `topic_<key>` (`existing: true`) or names `topic_<hub-name>_<key>`, and the cluster feeds `introduce-topic --topic-id <suggested_topic> --type <entity_type> --sources <members> --anchor <hub>` directly.
- `popular_source` is true when every edge is `extracted_from` (provenance, no clusters).
- `neighbors` is the raw `{id, rel, dir, title}` sample, empty unless `--sample N`.

### Reversible hygiene

The Dreamer never hard-deletes duplicates or stale items; both end as `kind = 'archived'` through the calibration archive, with a history row that keeps the prior kind.

- `mem merge --keeper K --lesser L [--body B | --body-file F] [--conflict]` updates the keeper body, archives the lesser (history `merge`) and links `L merged_into K` (or `K supersedes L` with `--conflict`) in one transaction. Pinned, skill, rule and preference items are refused as the lesser. Dreamer merges are capped by `memory.dream_hygiene.merge_cap` (default 10 per UTC day), enforced server-side.
- `mem area merge <from> --into <to> [--dry-run]` moves all source items across scopes, changing only their area. Preview returns counts per kind without writing. Empty, missing, equal and underscore-prefixed areas are refused; a new destination renames an area. Each item records one `update` with `area_merge` undo data in its history reason. `mem restore <id>` undoes its latest area merge; `--version <history-row>` selects an older merge without changing its body. Use `mem taxonomy` to list areas and counts. REM receives up to 20 conservative name pairs with counts and three sample titles each; it leaves uncertain pairs alone. The host caps area merges at 3 per dream worker run and saves a summary diary entry for each.
- `mem restore L` on an archived or merged item restores the prior kind (plus the `--version` body) and invalidates the merge relation.
- `mem calibration stale [--dry-run] [--limit N]` archives items older than 365 days with no direct touch (access, update, history row), no relation created or invalidated, and no touched active neighbour in that year. Pinned, skill, rule, preference, archived and importance >= 4 are excluded. The nightly run applies it only when `memory.dream_hygiene.stale_archive` is on (default off), capped by `stale_archive_cap` (default 20); the dry run lists ids and reasons and changes nothing.
- The diary log records `MERGED:` and `ARCHIVED:` lines with ids and reasons, and the worker report repeats them.

### Deadline finalization

A dreaming worker that hits its hard deadline is finalized by the host with finalization `partial` and reason `deadline`. A deadline-cut diary remains partial; complete diaries and a finalization the agent already recorded are preserved.

## Session hooks and memory

Claude hooks bind the current session and report permission/completion events. Raw terminal export has been removed: compaction does not create memory items. Consented integration repair removes only the retired Domios-owned PreCompact exporter and preserves foreign hooks.

| Hook | Endpoint | Purpose |
|---|---|---|
| `Notification` | `POST /api/tabs/{id}/hook/permission` | Surface a permission prompt without waiting for PTY auto-detect |
| `Stop` | `POST /api/tabs/{id}/hook/stop` | Mark task complete, fire `agent.task_complete` webhook |
| `SessionStart` | `POST /api/tabs/{id}/hook/session_start` | Track the live `session_id` so `/new` and `/clear` inside the TUI don't desync the persisted resume id |

Explicit `mem save` remains available with its 1 MB body cap. Provider transcripts remain in the provider session store.

Hooks are installed through the provider integration action into `~/.claude/settings.json`. The action shows the target and proposed changes, records consent, makes a recoverable backup, and preserves foreign hooks.

## Related documents

Skills and background services: `domios docs MEMORY-SERVICES`. Agent behavior and command reference: `domios docs MEMORY-USAGE`.

## Retired raw-export cleanup

After updating the owning peer's daemon and CLI, preview an already verified and deleted record locally with `domios mem purge-precompact <id>`. Apply with `--apply --verified-deleted`; `--vacuum` compacts FTS/SQLite and requests current-WAL truncation.

The prior deletion receipt must establish conversation kind, `flush:<tab> pre-compact` title, matching tab/pane source, PTY scrollback summary and flush/conversation tags. History does not retain source/summary/tags: `--verified-deleted` attests that prior receipt, not newly reconstructed provenance. The command refuses live, renamed, promoted, merged, unknown or oversized records. It purges only their history, exact vectors/FTS rows and attributable unshared body blobs. Shared blobs and graph knowledge remain. A payload-free receipt supports retry after file failure; partial cleanup is an RPC error with its report.

Check row/byte counts and compaction outcomes. Blob unlink, VACUUM and WAL checkpoint reclaim storage, without secure erasure. Backups, snapshots, old WAL copies, filesystem free space and provider transcripts remain untouched. No global orphan sweep runs.

## Memory backups

`domios mem backup create --out DIR` creates a timestamped local archive while the daemon runs: an online consistent SQLite snapshot of `mem/knowledge.db`, plus copies of `mem/blobs/` and `storage/`, protected by a SHA-256 manifest. Output must be outside the instance data directory. Incomplete archives are not published. File copies are not a cross-store transaction; quiesce file writers when that consistency is required. The archive excludes credentials, configuration, Neighbor keys and model caches.

Stop the daemon before `domios mem backup restore --from ARCHIVE`. Restore holds the daemon's filesystem ownership locks, verifies the manifest and SQLite integrity, replaces memory/storage and retains the previous trees in the printed private `.before-restore-*` directory. An active daemon refuses restore even if HTTP is down. Existing `domios mem restore ITEM_ID` restores one item's history.

Schedule online creation with `domios cron create memory-backup --schedule "0 3 * * *" --task "Run domios mem backup create --out /absolute/backup-directory once and report its result. Never restore or delete archives."` Choose a private output directory and copy completed archives to encrypted off-machine storage. For a Postgres backend use `pg_dump --format=custom` and, with the daemon stopped, `pg_restore --single-transaction --exit-on-error`; back up storage separately with its writers quiesced. Use protected PostgreSQL service/password files rather than passwords in argv.

<!-- revision: 60 -->
