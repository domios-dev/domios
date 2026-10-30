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

Set `memory.backend` to `"sqlite"` (default) or `"postgres"`. Postgres requires a compatible build and its URL in the active secret store as `domios.mem:postgres_url`, never config/env. Missing/empty/unavailable credentials refuse startup, with no fallback. Selecting an empty backend while the other configured store has items also refuses.

Stop the daemon, then explicitly migrate:
~~~bash
domios mem backend migrate --to postgres
domios mem backend migrate --to sqlite
~~~

The installed CLI pins the instance and verified server resource hint; missing/stale hints require reinstall, never PATH guessing. The server holds instance ownership locks and refuses a running daemon. Source is always opposite --to, read-only and retained; outdated sources must first be opened by their own Domios. Migration never starts/stops the daemon or changes config.

All 21 tables (history/authors/graph/vectors/bodies) copy in bounded pages and verify per-table counts. Nonempty targets need explicit --into-nonempty; row identity conflicts still refuse. A populated target with a different embedding stamp refuses. A differing target/new configured embedder rebuilds a shadow index, verifies vectors separately and reports counts; remote embedding still requires consent/credentials. Embedding failure retains source and reports failure.

Only after verified success, select the destination backend and start Domios. Failed/interrupted imports can leave partial targets: retain source and retry with a fresh target. Offline migration needs the configured secret store available; provision plugin-only vault access before migrating.

## Storage layer

### Item fields

- `kind` — `fact` | `learning` | `note` | `conversation` | `skill` | `rule` | `preference` | `peer` | `daily` | `summary`
- `scope` — `global` | `workspace:<id>`; any other value is refused
- `area` — project/topic tag (`auth`, `billing`, `deploy`, `home`, `_dreams`)
- `importance` — 1-5; the main wake-up ranking term
- pinned (`mem pin <id>` / `mem unpin <id>`) — pinned items always load in wake-up, subject to the budget

Memory items and history carry an author: owner, neighbor, or system. Neighbor authors carry the registered friend's account UID and friendship ID, resolved by the daemon from authenticated friendship data. Request fields cannot choose an author. Replacement re-signs the item while history retains the prior author and body.

A concierge may save only through `domios mem neighbor-save --kind fact --area <granted-area> --title "<title>" --from-pad <body.md>` (also kind `note`, optional `--summary` and repeated `--tag`). The live `memory_write_areas` grant defaults to empty. The daemon forces global scope and unpinned content, clamps importance to 1–5, scans all content fields for credentials, and enforces byte caps of 200 title / 16384 body / 32752 total. Matching kind/title replacement requires a granted area and an unpinned target; conflict refuses without exposing the item. Revocation stops the next save and retains past authorship. These saves never schedule extraction and are excluded from wake-up/promotion; all other concierge memory mutations remain denied.

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

  Alternatively use `domios mem secret save --name openai-embedder` with the key on stdin. For setup, pipe the supplied key into the dedicated key-set command, then select that embedder; never place it in argv, output, logs or mem.

### Supporter Agent memory re-rank

A connected adapter automatically grades a widened hybrid candidate pool and orders hits by relevance. Disconnected search stays plain recall. Use `domios mem search --supporter on|off` for one diagnostic request; combining `--supporter on` with legacy `--rerank` refuses.

Applied hits show relevance 0..1 (human: rel 92%). CLI JSON returns id/doc pointer, source, optional via, baseline score and relevance, omitting item titles/bodies/summaries/tags; use `domios mem get <id> --json` for the full record. HTTP search retains item detail for the UI.

The supporter envelope is applied (adapter, low_confidence, max_relevance), fallback (reason: missing adapter, failure, deadline, budget or credits), or skipped (legacy_rerank). Fallback returns unchanged recall hits; without the key the plain render is unchanged. low_confidence means the best grade is below the configured threshold: weak leads, not answers.

`[supporter.memory_rerank.sources]` controls:
- recall: default 30 candidates;
- graph: default 3 extra live items linked by active relations to keyword-matched entities; SQL ranks by matched entities, importance and recency, deduplicating recall hits;
- docs: 0..10 sections, default 3; keyword order unless docs_hybrid enables available semantic vectors, with keyword fallback;
- sessions: 0..5 recent snippets, default 0; provider errors, timeouts or saturation skip this source within the rerank deadline.

Applied graph/docs sources add via and pool counts. Both graph=0 and docs=0 omit this metadata; fallback omits all additions. Doc hits expose doc/section/anchor/pointer/relevance and run whole-doc/help commands. Session hits expose session_id/provider/title/excerpt/pointer/relevance and point to `domios sessions get <id>`. Docs/sessions appear only after successful reranking and never record mem access. Pool grading records no access; only returned memory hits do, once each.

Adapters declare required/optional/no token. Save required tokens through piped stdin with `domios supporter token set --adapter <adapter>`; never argv/config. Terminal input and --mesh refuse. `token status` returns presence/path/mode_ok, never values. Tokens live in each instance's owner-only supporter directory (Unix directory 0700/file 0600; Windows retains only narrow runtime grants). None-mode adapters ignore writes.

### Supporter Agent

The Supporter reranks memory, observes tab lifecycle events and supports bounded General Agent recovery. Connection enables it automatically; disconnection preserves deterministic behavior. Discover with `domios supporter models`, connect with `domios supporter use <adapter>`, inspect `domios supporter status` and disconnect with `domios supporter disconnect <adapter>`. Settings > Agents > Supporter Agent retains multiple connections/models; removing the active one selects no replacement.

calls_per_hour is unlimited when absent/0; positive values up to 100000 count physical HTTP attempts (batches once, sequential calls separately; local decisions are free). At budget, search falls back and observation skips until capacity returns. Status shows budget_reached/next free time, calls_per_hour:null when unlimited, activity, verdicts and spend. Default stage deadline: 3000 ms. Missing prices yield no estimate; credit exhaustion emits one failure alert and deterministic fallback until a later successful probe.

Queries/candidate text (default cap 2000 characters), configured docs/session excerpts and recent screen/chat/runtime context may reach the adapter. Jev runs at TypeSafe; local adapters use their configured server. Tokens remain local. Jev's calibrated choice/noul probabilities differ from sampled-text logprobs; generic/local models have no calibrated confidence.

On agent/Team/worker/cron tabs, a calibrated choice >=0.80 may answer only provider-declared folder-trust/tool-permission/update choices. Failed/capped/other/low-confidence decisions use the structural responder; unresolved cases notify the owner. Owner tabs retain prompts. Never auto-answer authentication/login/model-unavailable/quota prompts. See `domios docs TAB-LIFECYCLE`.

General Agent recovery allows at most two restarts per episode, then owner action. Crash recovery requires a verified session and an absent provider process; startup failures/error loops each allow one attempt within that cap. Model-authorized recovery stays notify-only pending qualification. See `domios docs AGENT-CONTROL`.

Mission Control shows verdicts. Bounded supporter/journal.jsonl stores action/budget/answer/model/criteria metadata, never raw inputs/tokens. other causes no action/hint. Clearing a required token preserves the connection but makes it unusable. The Guide opens from a standalone `<ds-guide section="supporter-agent" />` paragraph. Connection guidance: `domios docs SPINAL-CORD`; settings: `domios docs RUNTIME-CONFIG`; response fields/routes: `domios docs API-MEMORY`.

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

`domios mem graph hubs --threshold 20 --limit 5 --sample 0` is local/read-only. It returns highest-degree active hubs, total_hubs before limiting, excluded dreams anchor project_dreams, and existing_anchors. Per-hub fields include id/degree/rel_dist/popular_source/sub_anchors/outbound_only/unclustered/clusters/neighbors.

Clusters split inbound neighbors by area, title keyword and kind, never repeat the hub's name, and reuse existing topic_/role_ sub-anchors. suggested_topic/entity_type/members feed introduce-topic --topic-id/--type/--sources/--anchor. popular_source means all edges are extracted_from provenance; no clusters. Raw neighbors {id,rel,dir,title} appear only with --sample N. Inspect command --help for field/option detail.

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

`domios mem backup create --out DIR` publishes a timestamped local archive with an online-consistent SQLite knowledge.db snapshot, mem/blobs/ and storage/, protected by a SHA-256 manifest. Output must be outside the data directory; incomplete archives never publish. Copies are not cross-store transactions: quiesce file writers when required. Credentials/config/Neighbor keys/model caches need separate protected backups.

Stop the daemon before `domios mem backup restore --from ARCHIVE`. Restore holds the same ownership locks, verifies manifest/SQLite integrity, replaces memory/storage and retains the old trees in the printed private .before-restore-* directory. Active daemons refuse even with HTTP down. `domios mem restore ITEM_ID` remains single-item history restore.

Use a Domios cron for online creation and encrypted off-machine archive storage. Postgres needs pg_dump --format=custom and, with the daemon stopped, pg_restore --single-transaction --exit-on-error; back up storage with writers quiesced. Supply database credentials through protected service/password files, never argv.

<!-- revision: 62 -->
