# API transport protocols

Output cursors, streaming, examples, and the socket JSON-RPC protocol.

### Output Cursor Pattern

Use `offset` parameter for efficient polling without fetching the full buffer:
```bash
# First call — get all output
curl "http://host:7685/api/tabs/$TAB/output"
# Response: {"output": "...", "offset": 1234}

# Subsequent calls — only new data
curl "http://host:7685/api/tabs/$TAB/output?offset=1234"
# Response: {"output": "new stuff", "offset": 1500}
```

### ANSI Stripping

Add `strip_ansi=true` to get clean text without escape codes:
- `GET /api/tabs/:id/output?strip_ansi=true`
- `POST /api/tabs/:id/exec {"command": "...", "strip_ansi": true}`

### Exec Pattern

Send a command and wait for its output in one call:
```bash
curl -X POST "http://host:7685/api/tabs/$TAB/exec" \
  -H "Content-Type: application/json" \
  -d '{"command": "echo hello", "timeout_ms": 5000}'
# Response: {"output": "hello\r\n...", "offset": 1600}
# If timeout: {"output": "...", "offset": 1600, "timeout": true}
```

### SSE Output Stream

Subscribe to real-time output via Server-Sent Events:
```bash
curl -N "http://host:7685/api/tabs/$TAB/output/stream"
# data: {"output": "new text", "offset": 1234}
# data: {"output": "more text", "offset": 1300}
```

### Examples

```bash
# Health check
curl http://host:7685/api/health

# Create workspace + tab
WS=$(curl -s -X POST http://host:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name":"my-project"}' | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")

# Get tab and run command
TAB=$(curl -s http://host:7685/api/tabs | python3 -c "import sys,json;print(json.load(sys.stdin)[-1]['id'])")
curl -X POST "http://host:7685/api/tabs/$TAB/exec" \
  -H "Content-Type: application/json" \
  -d '{"command": "npm test", "timeout_ms": 30000}'

# Send with batch (e.g. launch Claude Code)
curl -X POST "http://host:7685/api/tabs/$TAB/send" \
  -H "Content-Type: application/json" \
  -d '{"batch": [{"text": "claude"}, {"text": "", "delay_ms": 8000}]}'

# Register webhook for agent events
curl -X POST http://host:7685/api/webhooks \
  -H "Content-Type: application/json" \
  -d '{"url": "https://my-orchestrator/hook", "events": ["agent.permission_request"]}'

# One-shot webhook (fires once, then auto-deletes — like a Promise)
curl -X POST http://host:7685/api/webhooks \
  -H "Content-Type: application/json" \
  -d '{"url": "https://my-orchestrator/done", "events": ["agent.task_complete"], "once": true, "tab_id": "'"$TAB"'"}'

# Network info
curl http://host:7685/api/network
# {"local_ip":"192.168.1.195","tailscale_ip":"100.84.159.37","api_url":"http://192.168.1.195:7685"}
```

#### Updates
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/update/status` | — | Current updater state and last update result |

#### Git
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/git/graph` | — | Commit graph for configured repositories |
| GET | `/api/git/commit/{sha}/files` | — | Files changed by a commit |
| GET | `/api/git/commit/{sha}/diff` | — | Diff for a commit |
| GET | `/api/git/workdir-diff` | — | Current working tree diff |
| GET | `/api/git/workdir-files` | — | Current working tree changed-file list |
| POST | `/api/git/revert-file` | `{path}` | Revert one file through the API guardrails |
| GET | `/api/git/repos` | — | Known repositories |

## Socket API (JSON-RPC v2)

### Connection
- **Windows**: Named pipe `\\.\pipe\codeterm-{PID}`
- **macOS/Linux**: Unix socket `/tmp/codeterm-{PID}.sock`
- Auto-discovery: `CODETERM_SOCKET_PATH` env var

### Protocol
Newline-delimited JSON:
```json
{"jsonrpc":"2.0","id":1,"method":"system.identify","params":null}
```

### Methods

| Method | Params | Description |
|--------|--------|-------------|
| `system.identify` | — | Version, platform, features |
| `system.capabilities` | — | List all methods. Socket-only; unsupported over mesh relay. |
| `workspace.list` | — | List workspaces |
| `workspace.create` | `{name}` | Create workspace |
| `workspace.focus` | `{workspace_id}` | Focus workspace |
| `workspace.rename` | `{workspace_id, name}` | Rename workspace |
| `workspace.close` | `{workspace_id}` | Close workspace |
| `workspace.recently_closed` | `{workspace_id}` | List recently-closed tab entries for a workspace. Returns `ClosedTab[]`. |
| `workspace.reopen_closed` | `{workspace_id, index?}` | Reopen the nth recently-closed tab in a workspace (`index` 0 = most recent). Returns `TabInfo`. |
| `workspace.reopen_closed_group` | `{group_id}` | Reopen a closed group by `group_id`. Returns `{workspace_id, tab_id, tab_id}`. |
| `workspace.move_tab` | `{tab_id, from_workspace_id, to_workspace_id}` | Move tab |
| `team.list` | — | Return `{teams: Team[]}` from the live Team registry. |
| `team.show` | `{team_id}` | Return `{team: Team, members: [{tab_id, title, role, anchors_team}]}` (members sorted by tab id) or reject an unknown Team. |
| `team.rename` | `{team_id, name, caller_tab_id?}` | Rename a Team and emit `mux-state-changed`. |
| `team.close` | `{team_id, caller_tab_id?}` | Explicitly cascade-close Team members, remove the registry entry, archive/detach bound plans, retire owner/planner routing, and emit `mux-state-changed`. |
| `team.adopt_plan` | `{team_id, plan_id, caller_tab_id}` | Atomically activate an existing plan 1:1 under the Team's sole live Orchestrator, replace owner/history, clear stale planner routing, relink hierarchy, and emit both `plan-changed` and `mux-state-changed`. The caller is required and must manage the destination Team. |
| `team.adopt_agent` | `{team_id, tab_id, caller_tab_id}` | Attach an already-running ROLELESS agent tab to a Team, binding it under the Team's live Orchestrator, and emit `mux-state-changed`. Returns `{team_id, tab_id, team}`. |
| `plan.transfer` | `{plan_id, to: OrchestratorRef, reason?, caller_tab_id}` | Transfer atomically to the destination Orchestrator's live Team. The caller is required and must manage the source Team. |
| `tab.list` | — | List tabs |
| `tab.focus` | `{tab_id}` | Focus tab |
| `tab.close` | `{tab_id}` | Close tab |
| `tab.send_text` | `{tab_id, text}` | Send a text message to PTY; submission is mandatory |
| `tab.get_output` | `{tab_id, max_bytes?}` | Read terminal output |
| `tab.rename` | `{tab_id, title}` | Rename tab (pinned) |
| `mem.stats` | — | Return memory item, vector, entity, and relation counts over the daemon socket. |
| `mem.search` | `{q, kind?, area?, scope?, project?, limit?, rerank?}` | Search memory and the optional project plan index over the daemon socket. |
| `mem.get` | `{id}` | Fetch one mem item by id. |
| `mem.save` | `{kind, title, body, scope?, summary?, area?, tags?, importance?, source_path?, replace?, skip_extract?, pinned?}` | Create a mem item; may spawn an auto-extraction worker. |
| `mem.delete` | `{id}` | Delete a mem item by id. |
| `mem.forget` | `{area}` | Batch-delete every item in an area. |
| `mem.update` | `{id, title?, summary?, area?, tags?, body?, importance?, pinned?, changed_by?}` | Partial update; also used for pin/unpin (`pinned: true/false`). |
| `mem.pinned` | `{limit?}` | List pinned items. |
| `mem.append` | `{id, text, changed_by?}` | Append text to an item's body (non-destructive). |
| `mem.restore` | `{id, version?, changed_by?}` | Restore a prior body from the audit history. |
| `mem.wake_up` | `{budget?, l0_only?, no_l0?, explain?, workspace?}` | L0+L1 wake-up context text. |
| `mem.learn` | `{text, kind?, tags?}` | Capture a quick learning. |
| `mem.learn_promote` | `{id}` | Promote a learning to a fact. |
| `mem.flush` | `{tab, title?}` | Flush a tab's PTY scrollback as a `conversation` mem item. |
| `mem.sync` | `{path?}` | Trigger file sync for source-tracked items (Librarian). |
| `mem.reembed` | `{scope?}` | Re-embed mem items and/or graph entities. |
| `mem.extract_pending` | `{limit?}` | Re-queue extraction workers for un-extracted items. |
| `mem.history` | `{id}` | Full audit trail for one item. |
| `mem.taxonomy` | `{workspace?}` | `{area -> {kind -> count}}` breakdown. |
| `mem.tail` | `{since?, limit?}` | Recent mem writes, newest first. |
| `mem.calibration.log` | `{since?, limit?, count?}` | Calibration audit log (importance/archive changes). |
| `mem.calibration.sweep` | `{force?}` | Trigger the dreamer's importance/archive sweep. |
| `mem.worker.spawn` | `{intent, task, agent?, parent?}` | Spawn a hidden graph worker. |
| `mem.worker.list` | — | List all graph workers. |
| `mem.worker.get` | `{id}` | Fetch one graph worker by id. |
| `mem.worker.progress` | `{id, text, status?}` | Report worker progress (called by the worker itself). |
| `mem.worker.report` | `{id, summary}` | Submit a worker's final report and mark it done. |
| `mem.worker.kill` | `{id}` | Kill a graph worker. |
| `mem.graph.create` | `{id, type, scope?, properties?}` | Create a graph entity. |
| `mem.graph.relate` | `{from, rel, to, valid_from?, valid_to?, confidence?, scope?, reason?}` | Create a relation. |
| `mem.graph.related` | `{id, rel?, as_of?, include_invalidated?}` | List outgoing relations from an id. |
| `mem.graph.invalidate` | `{from, rel, to, ended?}` | Invalidate a relation (preserves history). |
| `mem.graph.timeline` | `{entity, include_invalidated?}` | Full temporal timeline for an entity. |
| `mem.graph.traverse` | `{from, hops?, max_nodes?, as_of?, include_invalidated?}` | BFS traversal from an entity. |
| `mem.graph.path` | `{from, to, max_hops?, include_invalidated?}` | Shortest path between two entities. |
| `mem.graph.tunnels` | `{include_invalidated?}` | List entities bridging 2+ areas. |
| `mem.graph.alias` | `{alias, canonical}` | Merge one entity into another. |
| `mem.graph.repair.find_relations` | `{group_by, limit?}` | Find duplicate relations (read-only). |
| `mem.graph.repair.delete_relations` | `{rowids, actor?}` | Delete relations by rowid (audited). |
| `mem.graph.repair.merge_relations` | `{keep, merge, actor?}` | Merge relations into `keep`. |
| `mem.graph.repair.find_entities` | `{strategy, limit?}` | Find duplicate entities. |
| `mem.graph.repair.augment_entity` | `{id, patch, actor?}` | Patch properties on an entity (audited). |
| `mem.graph.repair.delete_entity` | `{id, force?, purge_history?, actor?}` | Delete an entity (audited). |
| `mem.graph.repair.orphan_vectors` | `{dry_run?, actor?}` | Find/delete orphan `mem_vectors` rows. |
| `mem.graph.repair.prune_thin_vectors` | `{dry_run?, actor?}` | Find/delete bodyless-embedding vectors. |
| `mem.graph.repair.collapse_synonym_edges` | `{dry_run?, actor?}` | Collapse parallel membership-synonym edges. |
| `mem.graph.repair.prune_dream_hub_edges` | `{dry_run?, actor?}` | Invalidate dream-diary backlinks into hubs. |
| `mem.graph.repair.orphan_blobs` | `{dry_run?, actor?}` | Find/delete unreferenced blob files. |

`codeterm mem` routes its full item/history/taxonomy/tail/learn/wake-up/sync/skill/worker/calibration/graph surface over these socket methods — HTTP is no longer attempted for any of it, so a live socket with a dead HTTP API no longer breaks `mem`. A handful of `mem graph` subcommands (route, purge-invalidated, revive-broken, introduce-topic, hubs, islands, orphans, get) are local-only by design — the CLI opens `knowledge.db` directly and never touches either transport. `mem secret` is the one deliberate holdout: `mem.secret.*` gets an explicit `-32004` rejection over the socket (see `secret_socket_rejection` in `socket/server/mem/mod.rs`) because it's the only mem surface that can return plaintext credentials and the socket has no auth gate, so it stays on the bearer-token-gated `/api/secrets/*` HTTP API by design, not because it's unmigrated.
| `notification.create` | `{title, body?, level?, caller_tab_id?, tab_id?, category?, event_key?}` | Create notification. Both ids are hints: the daemon resolves the caller first, falls back to `tab_id`, and stores navigation provenance (`tab_id`, `tab_title`, `workspace_id`) only for a tab that is still live. An unresolvable or closed tab is dropped and the notification still lands without a Show Tab target. `workspace_id` is derived, never accepted. `category` is one of `milestone`, `blocker`, `failure` — any other value is rejected with `-32602`, and an omitted category means local-only (no Telegram push). `event_key` is a coalescing hint; the daemon derives one when it is absent. |
| `notification.list` | `{unread_only?}` | List notifications |
| `notification.clear` | `{id?}` | Clear notifications |
| `agent.list` | — | List all tabs with roles |
| `agent.hierarchy` | — | Full hierarchy tree |
| `agent.spawn` | `{agent?, title?, cwd?, role?, parent_tab_id?, can_spawn?, task?, args?, workspace_id?}` | Spawn agent (same as REST) |
| `agent.set_role` | `{tab_id, role, parent_tab_id?, can_spawn?}` | Set tab role |
| `agent.clear_role` | `{tab_id}` | Clear tab role |
| `agent.status` | `{tab_id}` | Get tab status + role |
| `agent.children` | `{tab_id}` | List child tabs |
| `agent.link` | `{tab_id, parent_tab_id}` | Link tab under parent tab |
| `agent.report` | `{tab_id, message}` | Send report to direct parent |
| `agent.restart` | `{tab_id, force?, grace_secs?}` | Restart the agent in a tab; resumes only when the recorded session is valid. |
| `plan.task.attempts` | `{plan_id, task_id}` | List execution attempts for one plan task |
| `plan.task.attempt.show` | `{plan_id, task_id, attempt_id}` | Show one historical task attempt |
| `plan.task.respawn` | `{plan_id, task_id, attempt_id, provider?, model?, parent_tab_id?, workspace_id?, title?}` | Spawn a new Worker from one prior task attempt and record a new live attempt |
| `cron.list` | `{workspace_id?}` | List cron jobs |
| `cron.get` | `{id}` | Get cron job by ID |
| `cron.create` | `{name, schedule, task, workspace_id, agent?, model?, cwd?, description?, oneshot?, jitter_max_ms?}` | Create cron job |
| `cron.update` | `{id, name?, schedule?, task?, cwd?, description?, enabled?, agent?, model?, jitter_max_ms?}` | Update cron job |
| `cron.delete` | `{id}` | Delete cron job |
| `cron.enable` | `{id}` | Enable cron job |
| `cron.disable` | `{id}` | Disable cron job |
| `cron.trigger` | `{id}` | Manually trigger cron job |
| `cron.cancel` | `{id}` | Stop currently-running executions (`{cancelled: N}`) without deleting the job |
| `cron.history` | `{id}` | Get run history |
| `cron.complete` | `{run_id, summary?}` | Mark a cron run as complete |
| `services.list` | `{kind?, enabled?}` | List registered services (cron + daemon). |
| `services.create` | `{name, kind:"daemon", health_mode, ...}` | Register a daemon-kind service. Same body shape as `POST /api/services`. Returns `{service, token}`; rejects non-daemon kinds with `-32602` (use `cron.create` for cron jobs). |
| `services.get` | `{id}` | Get one service (with latest health snapshot). |
| `services.update` | `{id, ...patch}` | Sparse update; same fields as `PUT /api/services/{id}`. |
| `services.delete` | `{id}` | Delete service + its push token + activity. |
| `services.regenerate_token` | `{id}` | Rotate push token. Returns `{token}`. |
| `activity.list` | `{source?, since?, limit?}` | Read recent activity events (push / health / peer / cron). Defaults: `limit=500`. |
| `bridge.send` | `{text, chat_id?, service?, append?, type?, file?, caption?, filename?}` | Send message or media to external service (Telegram). `type`: `text`/`photo`/`document`/`voice`/`animation`/`video`/`sticker`. `file`: absolute path for media. `append`: concatenate to previous message. |

`team.adopt_agent` and the `agent.spawn` / `tab` creation `{team_id, creator_tab_id}` pair share one membership check: the caller tab must exist, already carry that `team_id`, sit in the Team's workspace, and the Team must have a live anchoring Orchestrator. Failing any of those refuses before the tab is created or mutated. `team.adopt_agent` additionally refuses a missing tab, a tab already bound to a Team, and a role-bearing tab — a role- or plan-task-bound spawn derives its Team from that binding, so `agent.spawn` rejects `team_id` combined with `role` or `plan_task`.

For `team.rename` and `team.close`, omitted `caller_tab_id` is the trusted direct-user path on the local socket. `team.adopt_plan` requires a non-empty caller belonging to the destination Team as its Orchestrator or planner-profile Manager; missing callers, Workers, and foreign tabs are rejected before mutation. Adoption preserves the structured `team_already_has_active_plan` conflict when the Team already owns another active plan. Events are emitted only after persistence succeeds.

<!-- revision: 4 -->
