# R5 prep: sentinel ports

Source-only prep. `src-tauri/crates/codeterm-sentinel-ports` (serde + serde_json only) defines the seam; nothing uses it yet and `sentinel` has not moved. R3 (cron clock/store, `crate::storage` constants, `crash::shutdown`) must land first.

## Ports

| trait | methods | replaces on `scheduler::Handle` |
|---|---|---|
| `SentinelWake` | `notify`, `notify_event_subscription_changed`, `terminal_output` | same-named methods |
| `SentinelCleanup` | `cancel_tab_owned_runs`, `remove_worker_watchers`, `remove_team`, `remove_workspace` | same-named methods |
| `BlueprintStore` | `records`, `scope_of`, `insert_restored`, `ensure_external_commit_ready` | `handle.store.lock().list()/get()`, `storage::commit(.., stage_insert)`, `ensure_external_commit_ready` |

`SentinelPorts` bundles the three as `Arc<dyn ..>`. The sentinel crate builds it from its `Handle` in `scheduler::start` (called at `startup/servers.rs:275`) and registers it with `app.manage(...)`; core code does `try_state::<SentinelPorts>()` where it now does `try_state::<scheduler::Handle>()`.

`BlueprintRecord` carries `id`, `workspace_id`, `team_id`, `scenario_root` and the full `Sentinel` as `definition: serde_json::Value`. The `Sentinel` struct is serde-flattened and deeply typed (`StageSource`, `Machine`, `Progress`, ...), so it stays in the sentinel crate; `close_history` persists the JSON value, which is byte-identical to what `Vec<Sentinel>` serialises to today.

## Call sites to replace (core -> sentinel, non-test)

| # | site | uses | port |
|---|---|---|---|
| 1 | `service/close_history.rs:115` | list jobs in scope for the closed-structure snapshot | `BlueprintStore::records` |
| 2 | `service/close_history.rs:168` | `store.get(id).is_none()` to detect change | `BlueprintStore::scope_of` |
| 3 | `service/close_history.rs:320` (+`:334`, `handle.notify()`) | restore: `get`, scope check, `storage::commit(stage_insert)` | `scope_of`, `insert_restored`, `SentinelWake::notify` |
| 4 | `service/orchestrator_digests/inventory.rs:22` | count job roots per team | `BlueprintStore::records` |
| 5 | `service/output_feed.rs:80` | `terminal_output` | `SentinelWake` |
| 6 | `service/plugin_host.rs:380` | `notify` | `SentinelWake` |
| 7 | `service/plugin_host.rs:440` | `notify_event_subscription_changed` | `SentinelWake` |
| 8 | `service/plugin_host.rs:454` | `notify_event_subscription_changed` | `SentinelWake` |
| 9 | `service/tab.rs:158` | `cancel_tab_owned_runs` | `SentinelCleanup` |
| 10 | `service/team.rs:587` | `remove_team` | `SentinelCleanup` |
| 11 | `service/team/storage_teardown.rs:61` (field `:16`, uses `:85`, `:133`) | `ensure_external_commit_ready`, `notify` | `BlueprintStore`, `SentinelWake` |
| 12 | `service/workspace.rs:565` | `remove_workspace` | `SentinelCleanup` |
| 13 | `socket/server/plan_review.rs:327` | `remove_worker_watchers` | `SentinelCleanup` (socket moves in R4 and uses the port too) |
| 14 | `api/debug_routes.rs:50` | `notify` | `SentinelWake` |

Sites 1-12 are the core's. 13 and 14 are the same pattern in other layers. `close_history.rs:45` (`blueprints: Vec<Sentinel>`) becomes `Vec<BlueprintRecord>`.

## Moves with the sentinel crate (not ports)

`service/sentinel*` (adapter, 1.8k), `service/blueprint_pipeline.rs`, `service/blueprint_portable.rs`, `service/supporter_tab_state/sentinel_request.rs` (`machine::SafeKind`), `api/automation_graph_routes.rs`, `api/transport_orchestration/node_files.rs`, `socket/server/blueprint.rs`, `storage/{migration,verification}.rs`. They use sentinel types directly and would depend on the sentinel crate; any that the core still reaches must go through a port in a follow-up.

## Lock ordering

No port method may be called with `mux`, `output_buf`, `screen_state`, `agent_tracker` or `pty` locked unless the call is today. Call sites 5 and 9 sit next to `mux.read()` blocks: keep the guard scope unchanged. The sentinel `store` lock stays inside the sentinel crate. Each port method is blocking and must not be called from the async runtime directly; sites 10 and 12 already run in `spawn_blocking` or cleanup paths.
