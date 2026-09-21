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
```
The updater and Git surfaces are not transport; they live in API-INTEGRATIONS.

## Socket API (JSON-RPC v2)

### Connection
- **Windows**: Named pipe `\\.\pipe\codeterm-{PID}`
- **macOS/Linux**: Unix socket `/tmp/codeterm-{PID}.sock`
- Auto-discovery: `CODETERM_SOCKET_PATH` env var

### Protocol
Newline-delimited JSON:
```json
{"jsonrpc":"2.0","id":1,"method":"system.identify","params":null,"protocol_version":1}
```

#### Protocol version skew

Every request envelope carries the control-plane `protocol_version` integer
(one value, embedded in both `cli/src/protocol.rs` and
`src-tauri/src/socket/server/protocol.rs`). Dispatch compares it before
routing: a client presenting an older integer — or none at all, which is
treated as the oldest protocol, never crashed on — receives one typed error
instead of method-level parse noise:

```json
{"code": -32002,
 "message": "client protocol v0, server requires >=1 — run ./build.sh cli",
 "data": {"type": "version_skew", "client_protocol": 0, "server_protocol": 1,
          "recovery": "./build.sh cli"}}
```

`recovery` is `./build.sh cli` on dev builds and `codeterm update install --auto`
on release builds. The CLI prints the daemon-composed message verbatim.
Same-version requests take the unchanged dispatch path with zero added
round-trips. `POST /api/mesh/rpc` accepts the same `protocol_version` field and
returns the same typed error; the mesh relay forwards it untranslated, so a
mismatched peer fails with the same actionable one-liner. A newer client
against an older daemon is not refused by this gate; a method the old daemon
lacks still fails as `-32601`, which the CLI enriches with both app versions.

**1.10.0 release note:** pre-1.10 CLIs and mesh peers send no
`protocol_version`, so the moment a 1.10 daemon starts, every pre-1.10
`codeterm` binary — including a mesh peer's relayed `agent report` — is
hard-refused with this typed version-skew error on every socket and mesh
dispatch until it is updated. There is no grace mode: update the CLIs on all
mesh peers together with the daemon (the error's `recovery` field carries the
exact command).

### Methods

| Method | Params | Description |
|--------|--------|-------------|
| `system.identify` | — | Version, platform, features |
| `system.capabilities` | — | List all methods. Socket-only; unsupported over mesh relay. |
| `workspace.list` | — | List workspaces |
| `workspace.create` | `{name}` | Create workspace |
| `workspace.focus` | `{workspace_id}` | Focus workspace |
| `workspace.rename` | `{workspace_id, name}` | Rename workspace |
| `workspace.close` | `{workspace_id, force?}` | Close workspace. Agent tabs require `force: true`. |
| `workspace.recently_closed` | `{workspace_id}` | List recently-closed tab entries for a workspace. Returns `ClosedTab[]`. |
| `workspace.reopen_closed` | `{workspace_id, index?}` | Reopen the nth recently-closed tab in a workspace (`index` 0 = most recent). Returns `TabInfo`. |
| `workspace.reopen_closed_group` | `{group_id}` | Reopen a closed group by `group_id`. Returns `{workspace_id, tab_id, tab_id}`. |
| `workspace.move_tab` | `{tab_id, from_workspace_id, to_workspace_id}` | Move tab |
| `team.list` | — | Return `{teams: Team[]}` from the live Team registry. |
| `team.show` | `{team_id}` | Return `{team: Team, members: [{tab_id, title, role, role_type, role_profile, anchors_team}]}` (members sorted by tab id) or reject an unknown Team. A Cartographer member reports `role: "cartographer"`, `role_type: "Manager"`, and `role_profile: "cartographer"`. |
| `team.rename` | `{team_id, name, caller_tab_id?}` | Rename a Team and emit `mux-state-changed`. |
| `team.close` | `{team_id, caller_tab_id?}` | Explicitly cascade-close Team members, remove the registry entry, archive/detach bound plans, retire owner/planner routing, and emit `mux-state-changed`. |
| `team.adopt_plan` | `{team_id, plan_id, caller_tab_id}` | Atomically activate an existing plan 1:1 under the Team's sole live Orchestrator, replace owner/history, clear stale planner routing, relink hierarchy, and emit both `plan-changed` and `mux-state-changed`. The caller is required and must manage the destination Team. |
| `team.adopt_agent` | `{team_id, tab_id, caller_tab_id}` | Attach an already-running ROLELESS agent tab to a Team, binding it under the Team's live Orchestrator, and emit `mux-state-changed`. Returns `{team_id, tab_id, team}`. |
| `team.add_member` | `{team_id, provider, role, caller_tab_id?, title?, task?, model?}` | Staff a Team with a `worker`, `planner`, `orchestrator`, or `cartographer` member — the same service entry the UI add-member action and `POST /api/teams/{team_id}/members` use. Returns `{tab_id, team, role_type, role_profile?, plan_takeover?, reused}`. A Cartographer is an optional singleton Manager: repeat requests return the existing tab with `reused: true`; Team close removes its tab and binding. An `orchestrator` is accepted only while the Team has no live Orchestrator, and it then takes the retained active Plan over through the tab-gone owner rebind; `plan_takeover` reports `{plan_id, owner_tab_id}`. A takeover failure fails the request and rolls back the spawned member/lane while preserving the Team and Plan owner/history. Unlinking the final Orchestrator retains the Team and Plan identity/history so this documented command can still name them. `caller_tab_id` is optional only for a replacement anchor; roles that need a parent still refuse without one. `team.add_member` is the only staffing path: the UI action, `POST /api/teams/{team_id}/members`, `codeterm team attach-cartographer <team> --provider <provider>`, and `codeterm agent spawn <provider> --team <id> --role Orchestrator` all land on it. Its structural refusals lead with an error-code constant (`codeterm-types::team::staffing`) and are logged with the Team, the requested role, and the cause: `team_orchestrator_already_live`, `team_member_role_unknown`, `team_role_requires_spawner`, `team_caller_unauthorized`, `team_caller_cannot_spawn`, `team_workspace_unresolved`. The at-most-one-Orchestrator invariant is a headcount of **live** anchors — a removed tab is gone from the multiplexer and a dead agent's identity is cleared on exit, so neither counts. |
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
| `mem.worker.report` | `{id, summary, diary?, finalization?, reason?}` | Record that a worker's execution ended. Diary finalization is a separate fact: pass `diary` plus `finalization` (`succeeded`/`partial`/`failed`) and `reason`. |
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
| `notification.create` | `{title, body?, severity?, caller_tab_id?, tab_id?, level?, event_key?}` | Create notification. Both ids are hints: the daemon resolves the caller first, falls back to `tab_id`, and stores navigation provenance (`tab_id`, `tab_title`, `workspace_id`) only for a tab that is still live. An unresolvable or closed tab is dropped and the notification still lands without a Show Tab target. `workspace_id` is derived, never accepted. `level` is one of `verbose`, `action_needed`, `failure` — any other value is rejected with `-32602`, and an omitted level means local-only (no Telegram push). `severity` (`info`/`warning`/`error`) is toast styling only and gates nothing. `event_key` is a coalescing hint; the daemon derives one when it is absent. The result is `{id}`, plus `unseen: {closed:[{surface,reason}]}` when NO surface will show the record — the bell indicator hidden by `notifications.show_bell = false`, no desktop toast admitted, and no push channel carrying it. It is information, not an error: the record was opened and the id is returned either way, and the key is absent whenever any surface is open. |
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
| `agent.report` | `{tab_id, message, caller_tab_id?, caller_session_id?, session_binding_nonce?, plan_id?, task_id?, status?, ...}` | Send a report through authenticated hierarchy resolution. A Plan-authorized task-less reporter supplies `plan_id` without `task_id`/`status`; success includes `plan_report:{persisted,plan_id,stride_id,task_id:null,task_transition:false}`. |
| `agent.ack` | `{tab_id, plan_id?, task_id?}` | A task-bound Worker acknowledges its live task attempt. The attempt assigned to `tab_id` is authoritative; optional plan/task fields do not override it. This ACK also resolves task startup supervision. |
| `plan.review.authorize-reporter` | `{plan_id, reporter_tab_id, class, caller_tab_id, session_id, session_binding_nonce}` | Live Plan-owner grant of the `review` or `canonical_qa` plan-evidence capability to the target tab's exact current session/binding. The target must be live and task-less. |
| `agent.restart` | `{tab_id, force?, grace_secs?}` | Restart the agent in a tab; resumes only when the recorded session is valid. |
| `agent.startup_ack` | `{caller_tab_id, attempt_id, binding_nonce}` | Non-task spawns acknowledge their own first startup turn. Accepted only when the caller tab still owns that PTY attempt and launch nonce. A task-bound tab is refused with `codeterm agent ack` as its required command. |
| `agent.startup_hold` | `{caller_tab_id, tab_id, attempt_id}` | Cancel automatic cleanup for one inspected startup attempt. Authority is the tuple `(creator_tab_id, team_id, child_tab_id, attempt_id)`: only the tab recorded as the attempt's creator may hold it, the child must still be running that exact attempt, and a pair-leased attempt must still sit in its leased Team. No role bypasses this - a General Agent may hold only the children it spawned. The result carries `authority: {creator_tab_id, child_tab_id, attempt_id, team_id}`; a refusal returns `-32603` whose `data` carries `{held:false, reason, caller_tab_id, child_tab_id, attempt_id, authority}` with `reason` one of `unknown_child_tab`, `no_supervised_attempt`, `creator_mismatch`, `attempt_replaced`, `team_mismatch`. |
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

A `team pair` mints its Team together with the two launch attempts it reserves, and holds a lease on that exact reservation. When a leased attempt starts, is held, or the pair rolls back, the lease is retired and the Team becomes an ordinary Team. When a leased attempt instead reaches its terminal startup deadline, the closing tab sweeps the lease and removes that exact Team - only while it is still forming, empty, and free of any member the reservation never claimed. Normal, active, adopted, manually created, nonempty, and mismatched Teams are never touched, and `team.close` is unchanged.

`team.adopt_agent` and the `agent.spawn` / `tab` creation `{team_id, creator_tab_id}` pair share one membership check: the caller tab must exist, already carry that `team_id`, sit in the Team's workspace, and the Team must have a live anchoring Orchestrator. Failing any of those refuses before the tab is created or mutated. `team.adopt_agent` additionally refuses a missing tab, a tab already bound to a Team, and a role-bearing tab — a role- or plan-task-bound spawn derives its Team from that binding, so `agent.spawn` rejects `team_id` combined with `role` or `plan_task`.

For `team.rename` and `team.close`, omitted `caller_tab_id` is the trusted direct-user path on the local socket. `team.adopt_plan` requires a non-empty caller belonging to the destination Team as its Orchestrator or planner-profile Manager; missing callers, Workers, and foreign tabs are rejected before mutation. Adoption preserves the structured `team_already_has_active_plan` conflict when the Team already owns another active plan. Events are emitted only after persistence succeeds.
<!-- revision: 22 -->
