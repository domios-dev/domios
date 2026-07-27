# Workspace Split Dependency Graph

Phase-0 audit for `src-tauri/src`, generated from the current `lane/dep-audit`
tree.

## Reproduction

```bash
python3 - <<'PY'
import pathlib,re,collections
root=pathlib.Path('src-tauri/src')
tops=set(['lib','main'])
for p in root.iterdir():
    if p.is_dir() and (p/'mod.rs').exists():
        tops.add(p.name)
    elif p.suffix=='.rs':
        tops.add(p.stem)
edge=collections.Counter(); ex={}
for f in sorted(root.rglob('*.rs')):
    rel=f.relative_to(root)
    src=rel.parts[0] if len(rel.parts)>1 else rel.stem
    for m in re.finditer(r'\bcrate::([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)', f.read_text(errors='ignore')):
        path=m.group(1); tgt=path.split('::')[0]
        if tgt not in tops or tgt==src:
            continue
        edge[(src,tgt)] += 1
        ex.setdefault((src,tgt), f'{rel} -> crate::{path}')
print('| From | To | Refs | Example |')
print('| --- | --- | ---: | --- |')
for (a,b),c in sorted(edge.items(), key=lambda kv:(-kv[1],kv[0])):
    print(f'| `{a}` | `{b}` | {c} | `{ex[(a,b)]}` |')
PY
```

Scope notes:

- Counted only `crate::top_level_module` references where the first segment is a
  real top-level file or module under `src-tauri/src`.
- Ignored intra-module references and re-export-only hits such as
  `crate::ServiceState`.
- Result: 642 Rust source files, 278 inter-module edges, 2,376 raw `crate::`
  references before filtering.

## Final Crate Map

The draft graph is directionally right, but the real graph shows that the cut
must be by submodule in two places: `service` and `agent`. Moving all of either
top-level module would drag app wiring back into domain crates.

| Crate | Modules to move | Dependencies | Extraction status |
| --- | --- | --- | --- |
| `codeterm-types` | `agent/chat_types.rs`, `agent/keys.rs`, `agent/models.rs`, `agent/status.rs`, `config/schema/**`, `mesh/types.rs`, plan/task DTOs from `plan/types.rs` and `service/plan/types.rs`, shared attachment/message-marker structs | `serde`, `schemars`, small std-only helpers | Needs type moves first. Do not move app-facing provider registries here. |
| `codeterm-util` | `lock_hold.rs`, `paths.rs`, `process_util.rs`, atomic write/path helpers currently in `service/plan/atomic_write.rs` where generic, slug/time helpers | `codeterm-types` only where unavoidable | Clean leaf once `paths` stops importing app-only dev-instance assumptions. |
| `codeterm-plan` | `plan/**`, `service/plan/**`, `service/hierarchy/plan_reminder_nudge.rs` | `codeterm-types`, `codeterm-util`; app supplies event/mem/agent ports | Blocked by `service/plan` calling app event emitters, mem search, provider skills, and socket plan read paths. |
| `codeterm-mem` | `mem/**` | `codeterm-types`, `codeterm-util`; optional app worker/event ports | Blocked by `mem -> cron`, `mem -> service`, `mem -> api`, and `mem -> pty` edges. |
| `codeterm-mesh` | `mesh/**` | `codeterm-types`, `codeterm-util` | Small blockers: `mesh -> api` event emission and `mesh -> user_last_activity`. |
| `codeterm-bridge` | `bridge/**`, `transcribe/**`, `chatpipeline/**` | `codeterm-types`, `codeterm-util`; app supplies outbound tab/service ports | Blocked by `bridge -> service/startup` and plugin capability registration edges. |
| `codeterm-pty` | `pty/**`, `multiplexer/**`, `session/**`, `dispatch/**`, `resize_owner.rs` | `codeterm-types`, `codeterm-util` | Blocked by `multiplexer/dispatch -> service/api` command execution and provider status reads. |
| `codeterm-plugin` | `plugin/**`, `chatpreprocessor/**`, `view.rs`, `view_host/**`, `glance/**`, `bubble/**`, `agent_commands/**` | `codeterm-types`, `codeterm-util`; app supplies host ports | Blocked by `plugin -> api/service/socket/mem/secrets` host API calls and tests. |
| app crate (`src-tauri`) | `api/**`, `commands/**`, `socket/**`, `startup/**`, non-plan `service/**`, `cron/**`, `logs/**`, `notification/**`, `update/**`, `liveness.rs`, `keepawake.rs`, `crash/**`, app state wiring | all domain crates | Remains the only Tauri crate and owns runtime wiring/events. |

## Crate-Level Coupling

Initial aggregation by the final map above:

| From | To | Refs | Meaning |
| --- | --- | ---: | --- |
| app | `codeterm-types` | 368 | Expected after DTO extraction. |
| app | `codeterm-plan` | 294 | Mostly app/socket/service plan APIs. |
| app | `codeterm-pty` | 230 | Expected command and Tab routing. |
| app | `codeterm-util` | 173 | Paths/process helpers used everywhere. |
| app | `codeterm-mesh` | 135 | Mesh REST/CLI surface. |
| `codeterm-plugin` | `codeterm-types` | 121 | Provider capability types. |
| app | `codeterm-mem` | 71 | Mem REST and supervisor wiring. |
| app | `codeterm-plugin` | 46 | Plugin commands and startup. |
| app | `codeterm-bridge` | 32 | Bridge routes/startup. |
| `codeterm-plugin` | app | 21 | Must be inverted before plugin extraction. |
| `codeterm-types` | app | 19 | Means `agent` is not pure types yet. |
| `codeterm-mem` | app | 18 | Must be inverted before mem extraction. |
| `codeterm-pty` | app | 16 | Must be inverted before pty extraction. |
| `codeterm-bridge` | app | 5 | Must be inverted before bridge extraction. |
| `codeterm-mesh` | app | 4 | Must be inverted before mesh extraction. |

## Blocking Cycles And Break Strategy

Every bidirectional crate edge below blocks a clean Cargo cut.

| Cycle | Current edge evidence | Break strategy |
| --- | --- | --- |
| app <-> `codeterm-types` | `agent -> service/api/cron/plugin/mem/plan`, app imports agent/chat/config types heavily | Split `agent` into pure `agent_types` and app/provider runtime. Move only DTOs first; keep registries, watchers, parsers with app/plugin until their ports exist. |
| app <-> `codeterm-plan` | `service -> plan` 262, `socket -> plan` 22, but `service/plan` reaches app events/mem/agent/socket | Extract `PlanEventSink`, `PlanSearchPort`, `PlanSkillPort`, and `PlanClock` traits owned by `codeterm-plan`; app implements them. Move socket read/route adapters to app. |
| app <-> `codeterm-mem` | `api -> mem` 37; `mem -> cron` 12, `mem -> service` 5, `mem -> api` 1, `mem -> pty` 1 | Replace cron/service/api calls with app-owned worker scheduling and `MemEventSink`; move ANSI cleanup helper used by mem into util or keep the worker adapter in app. |
| app <-> `codeterm-mesh` | app routes/commands call mesh; `mesh -> api` 2 and `mesh -> user_last_activity` 2 | Use `MeshEventSink` for UI broadcasts and pass last-activity data into mesh manager as a value/provider trait. |
| app <-> `codeterm-bridge` | app routes call bridge; `bridge -> service` 4 and `bridge -> startup` 1 | Bridge owns protocol state only. App provides `BridgePanePort` and startup test helpers stay in app integration tests. |
| app <-> `codeterm-pty` | app/socket/commands call pty/mux/dispatch; `multiplexer -> service` 12, `dispatch -> service/api` 4 | Move command execution adapters out of `dispatch` into app. PTY/mux emits typed events; app consumes them and calls services. |
| app <-> `codeterm-plugin` | app commands/startup call plugin; `plugin -> api` 8, `plugin -> service` 11, `plugin -> socket` 1, `plugin -> mem` 4 | Define plugin host traits in `codeterm-plugin` for events, service context, socket path, compute/mem host APIs. App implements; plugin tests use fake hosts. |
| `codeterm-plugin` <-> `codeterm-types` | plugin imports provider types 121; `agent/chatbackend/usage -> plugin` 4 | Move `CapabilityRegistry`-touching implementations out of type modules into plugin/app adapters. Types crate must not know plugin. |
| `codeterm-pty` <-> `codeterm-types` | `multiplexer/pty -> agent` 13; `agent -> multiplexer/pty` 9 | Move shared tab/session/status DTOs to types. Keep prompt/screen-state logic in pty or app, not in agent types. |
| `codeterm-bridge` <-> `codeterm-plugin` | plugin tests call chatpipeline/transcribe 12; chatpipeline/transcribe register plugin capabilities 4 | Put capability registration in app/plugin adapter modules; bridge exposes plain processors. |
| `codeterm-types` <-> `codeterm-util` | `secrets -> config` 10; `config/agent -> paths` 3 | `secrets` is not util. Move secret storage to app/plugin host or a later `codeterm-secrets`; keep util std-only. |

## Phase-2 Extraction Order

1. `codeterm-types`: highest inbound coupling and unlocks every other crate.
   Scope must be narrow DTO/type moves only; leave runtime modules in app.
2. `codeterm-util`: move `lock_hold`, `process_util`, generic path/atomic helpers
   after the type cut removes `config -> paths` and `secrets -> config` pollution.
3. `codeterm-plan`: largest coherent domain and the named success metric. First
   invert event/search/skill ports, then move `plan/**` plus `service/plan/**`.
4. `codeterm-mem`: next strongest domain. It becomes straightforward after
   worker scheduling and event emission are app ports.
5. `codeterm-mesh`: comparatively small blockers; good early validation that
   app-owned event sinks work across REST/CLI.
6. `codeterm-pty`: heavier because `dispatch` still mixes domain state with app
   service calls. Extract after plan/mem reduce app service pressure.
7. `codeterm-bridge`: depends on the host-port pattern from plugin/PTY and has
   protocol tests that should move with it.
8. `codeterm-plugin`: most boundary-sensitive cut. Do last, after provider DTOs
   and app host traits are stable; parity tests must move with plugin runtime.

## Full Module Edge List

| From | To | Refs | Example |
| --- | --- | ---: | --- |
| `service` | `plan` | 262 | `service/hierarchy/plan_reminder_nudge.rs -> crate::plan` |
| `service` | `agent` | 148 | `service/agent_restart.rs -> crate::agent::provider` |
| `plugin` | `agent` | 115 | `plugin/agent_claude/mod.rs -> crate::agent::provider::AgentProvider` |
| `api` | `mesh` | 88 | `api/auth.rs -> crate::mesh::types::AccessLevel` |
| `service` | `multiplexer` | 88 | `service/general_agent/blocker.rs -> crate::multiplexer::types::PendingPrompt` |
| `api` | `service` | 85 | `api/agent_routes.rs -> crate::service::SpawnAgentParams` |
| `socket` | `service` | 85 | `socket/server/agent.rs -> crate::service::ServiceState` |
| `service` | `prompts` | 71 | `service/cron.rs -> crate::prompts::CRON_REPORT_HINT` |
| `commands` | `service` | 69 | `commands/agent.rs -> crate::service::ServiceState` |
| `service` | `config` | 40 | `service/cron.rs -> crate::config::memory_config` |
| `api` | `mem` | 37 | `api/mem_routes/calibration.rs -> crate::mem::store::with_changed_by` |
| `api` | `dispatch` | 34 | `api/health_routes.rs -> crate::dispatch::dispatch` |
| `socket` | `dispatch` | 32 | `socket/server/pane.rs -> crate::dispatch::dispatch` |
| `api` | `agent` | 29 | `api/agent_routes.rs -> crate::agent::reports::ReportDetails` |
| `commands_mesh` | `mesh` | 28 | `commands_mesh.rs -> crate::mesh::discovered::resolve_discovered` |
| `service` | `api` | 26 | `service/boot_stage.rs -> crate::api::events::emit_everywhere` |
| `service` | `mem` | 25 | `service/general_agent/instructions.rs -> crate::mem::MemManager` |
| `api` | `config` | 24 | `api/bridge_routes.rs -> crate::config::save_config_pub` |
| `socket` | `plan` | 22 | `socket/server/plan.rs -> crate::plan` |
| `api` | `commands` | 18 | `api/agent_routes.rs -> crate::commands::InstallPaneResult` |
| `bridge` | `transcribe` | 18 | `bridge/manager/mod.rs -> crate::transcribe::TranscribeRegistry` |
| `chatpipeline` | `transcribe` | 18 | `chatpipeline/mod.rs -> crate::transcribe::TranscribeRegistry` |
| `service` | `chatbackend` | 18 | `service/mod.rs -> crate::chatbackend::ChatBackendRegistryHandle` |
| `service` | `pty` | 18 | `service/agent_restart.rs -> crate::pty::screen_state::screen_has_shell_prompt` |
| `commands` | `config` | 16 | `commands/agent.rs -> crate::config::ConfigState` |
| `socket` | `api` | 16 | `socket/server/canvas.rs -> crate::api::events::emit_everywhere` |
| `api` | `chatbackend` | 15 | `api/agent_routes.rs -> crate::chatbackend::ChatBackendProvider` |
| `lib` | `service` | 15 | `lib.rs -> crate::service::sync_watcher::install_bridge_rules_for_all` |
| `api` | `secrets` | 14 | `api/routes.rs -> crate::secrets::api::status` |
| `commands` | `agent` | 14 | `commands/agent.rs -> crate::agent::provider::current_platform_tag` |
| `mem` | `cron` | 12 | `mem/dreaming/schedule.rs -> crate::cron::store::CronStore` |
| `multiplexer` | `service` | 12 | `multiplexer/commands.rs -> crate::service` |
| `plugin` | `service` | 11 | `plugin/activity.rs -> crate::service::ServiceContext` |
| `service` | `paths` | 11 | `service/general_agent/instructions.rs -> crate::paths::data_dir` |
| `socket` | `bridge` | 11 | `socket/server/bridge.rs -> crate::bridge::manager::ensure_supported_service` |
| `api` | `plan` | 10 | `api/plan_routes/gates.rs -> crate::plan::peer` |
| `commands` | `dispatch` | 10 | `commands/cron.rs -> crate::dispatch::Command::GetOutput` |
| `lib` | `paths` | 10 | `lib.rs -> crate::paths::data_dir` |
| `multiplexer` | `agent` | 10 | `multiplexer/commands.rs -> crate::agent::registry::AgentRegistry` |
| `secrets` | `config` | 10 | `secrets/api.rs -> crate::config::load_config` |
| `service` | `plugin` | 10 | `service/agent_restart.rs -> crate::plugin::agent_claude::ClaudeProvider::new` |
| `api` | `delayed_send` | 9 | `api/delayed_send_routes.rs -> crate::delayed_send::DelayedMessageView` |
| `commands` | `attachments` | 9 | `commands/clipboard.rs -> crate::attachments::save_with_uuid` |
| `commands` | `plugin` | 9 | `commands/pane.rs -> crate::plugin::js_runtime::PluginManifest` |
| `multiplexer` | `session` | 9 | `multiplexer/state/snapshot/mod.rs -> crate::session::store::SessionSnapshot` |
| `plugin` | `paths` | 9 | `plugin/agent_claude/provider/sessions.rs -> crate::paths::canonical_cwd` |
| `service` | `process_util` | 9 | `service/health/process.rs -> crate::process_util::no_window_std` |
| `api` | `bridge` | 8 | `api/bridge_routes.rs -> crate::bridge` |
| `commands` | `mesh` | 8 | `commands/system_info.rs -> crate::mesh::discovery::tailscale_self_dns_name` |
| `liveness` | `crash` | 8 | `liveness.rs -> crate::crash::report::timestamp_slug` |
| `plugin` | `api` | 8 | `plugin/activity.rs -> crate::api::events::SystemEvent` |
| `plugin` | `chatpreprocessor` | 8 | `plugin/js_runtime/chatpreprocessor.rs -> crate::chatpreprocessor::ChatPreprocessorProvider` |
| `service` | `message_markers` | 8 | `service/messenger.rs -> crate::message_markers::Marker::new` |
| `api` | `multiplexer` | 7 | `api/agent_routes.rs -> crate::multiplexer::types::AgentRole` |
| `api` | `pty` | 7 | `api/dev_cast_routes.rs -> crate::pty::cast` |
| `commands` | `api` | 7 | `commands/pty.rs -> crate::api::events::emit_everywhere` |
| `commands` | `multiplexer` | 7 | `commands/pane.rs -> crate::multiplexer::commands::MuxState` |
| `commands` | `pty` | 7 | `commands/pty.rs -> crate::pty::manager::build_session` |
| `commands_mesh` | `service` | 7 | `commands_mesh.rs -> crate::service::ServiceState` |
| `lib` | `mem` | 7 | `lib.rs -> crate::mem::supervisor::Supervisor::new` |
| `mem` | `agent` | 7 | `mem/auto_extract.rs -> crate::agent::models::canonicalize_short` |
| `mem` | `config` | 7 | `mem/auto_extract.rs -> crate::config::memory_config` |
| `agent` | `multiplexer` | 6 | `agent/native_prompt.rs -> crate::multiplexer::types` |
| `bridge` | `message_markers` | 6 | `bridge/manager/mod.rs -> crate::message_markers::hook` |
| `chatbackend` | `agent` | 6 | `chatbackend/mod.rs -> crate::agent::chat_types::StructuredChat` |
| `commands_mesh` | `api` | 6 | `commands_mesh.rs -> crate::api::mesh_routes::pairing::do_approve` |
| `mem` | `prompts` | 6 | `mem/auto_extract.rs -> crate::prompts::MEM_EXTRACTION` |
| `plugin` | `chatpipeline` | 6 | `plugin/js_runtime/hot_reload/tests.rs -> crate::chatpipeline::run_text_stages` |
| `provider` | `agent` | 6 | `provider/registry/tests.rs -> crate::agent::chat_types::StructuredChat` |
| `api` | `paths` | 5 | `api/mem_routes/dream.rs -> crate::paths::data_dir` |
| `api` | `provider` | 5 | `api/agent_routes.rs -> crate::provider::ProviderRegistry` |
| `api` | `update` | 5 | `api/session_routes.rs -> crate::update::shutdown::persist_session` |
| `bridge` | `config` | 5 | `bridge/manager/mod.rs -> crate::config::schema` |
| `delayed_send` | `service` | 5 | `delayed_send.rs -> crate::service::ServiceState` |
| `dispatch` | `multiplexer` | 5 | `dispatch/mod.rs -> crate::multiplexer::state::MultiplexerSnapshot` |
| `liveness` | `mesh` | 5 | `liveness.rs -> crate::mesh::broadcast_announce` |
| `mem` | `process_util` | 5 | `mem/dreaming/recluster.rs -> crate::process_util::no_window_std` |
| `mem` | `service` | 5 | `mem/auto_extract.rs -> crate::service::ServiceContext` |
| `mesh` | `process_util` | 5 | `mesh/discovery.rs -> crate::process_util::no_window_std` |
| `plugin` | `multiplexer` | 5 | `plugin/agent_claude/provider/mod.rs -> crate::multiplexer::types::PendingPrompt` |
| `plugin` | `secrets` | 5 | `plugin/js_runtime/host_secret.rs -> crate::secrets::secret_file::SecretFile` |
| `plugin` | `transcribe` | 5 | `plugin/js_runtime/hot_reload/tests.rs -> crate::transcribe::TranscribeRegistry::new` |
| `service` | `bridge` | 5 | `service/mod.rs -> crate::bridge::manager::OutboundSender` |
| `service` | `socket` | 5 | `service/general_agent/lifecycle.rs -> crate::socket::server::socket_display_path` |
| `socket` | `multiplexer` | 5 | `socket/server/mod.rs -> crate::multiplexer::commands::MuxState` |
| `startup` | `config` | 5 | `startup/bridge.rs -> crate::config` |
| `startup` | `paths` | 5 | `startup/cli_install.rs -> crate::paths::data_dir` |
| `startup` | `service` | 5 | `startup/background.rs -> crate::service::ServiceState` |
| `api` | `commands_mesh` | 4 | `api/web_routes.rs -> crate::commands_mesh::fetch_peer_services_view` |
| `bridge` | `service` | 4 | `bridge/manager/mod.rs -> crate::service::general_agent` |
| `commands` | `chatbackend` | 4 | `commands/pane.rs -> crate::chatbackend::ChatSessionInfo` |
| `commands` | `paths` | 4 | `commands/plugins/cli.rs -> crate::paths::data_dir` |
| `plugin` | `agent_commands` | 4 | `plugin/js_runtime/agent_commands.rs -> crate::agent_commands::AgentCommandProvider` |
| `plugin` | `bubble` | 4 | `plugin/js_runtime/bubble.rs -> crate::bubble::BubbleProvider` |
| `plugin` | `chatbackend` | 4 | `plugin/js_runtime/chatbackend.rs -> crate::chatbackend::ChatBackendProvider` |
| `plugin` | `glance` | 4 | `plugin/js_runtime/glance.rs -> crate::glance::GlanceProvider` |
| `plugin` | `mem` | 4 | `plugin/js_runtime/host_compute/tests.rs -> crate::mem::workers::make_info` |
| `plugin` | `view_host` | 4 | `plugin/js_runtime/hot_reload/tests.rs -> crate::view_host::ViewRegistry::new` |
| `service` | `cron` | 4 | `service/cron.rs -> crate::cron::store` |
| `service` | `lock_hold` | 4 | `service/inject_verify.rs -> crate::lock_hold::TimedMutex` |
| `service` | `mesh` | 4 | `service/hierarchy.rs -> crate::mesh::types::RelayCommand` |
| `service` | `usage` | 4 | `service/mod.rs -> crate::usage::UsageStoreHandle` |
| `socket` | `notification` | 4 | `socket/server/mod.rs -> crate::notification::NotifState` |
| `startup` | `agent` | 4 | `startup/background.rs -> crate::agent::status::PaneStatus` |
| `startup` | `api` | 4 | `startup/background.rs -> crate::api::events::emit_everywhere` |
| `startup` | `multiplexer` | 4 | `startup/background.rs -> crate::multiplexer::commands::MuxState` |
| `update` | `service` | 4 | `update/post_update.rs -> crate::service::ServiceState` |
| `agent` | `pty` | 3 | `agent/run_watcher.rs -> crate::pty::screen_state::screen_has_shell_prompt` |
| `agent` | `service` | 3 | `agent/live_state.rs -> crate::service::ServiceState` |
| `api` | `cron` | 3 | `api/cron_routes.rs -> crate::cron::store::CronCreateParams` |
| `api` | `keepawake` | 3 | `api/network_routes.rs -> crate::keepawake::status` |
| `autostart` | `process_util` | 3 | `autostart.rs -> crate::process_util::no_window_std` |
| `chatpipeline` | `chatpreprocessor` | 3 | `chatpipeline/mod.rs -> crate::chatpreprocessor` |
| `commands` | `autostart` | 3 | `commands/system.rs -> crate::autostart::is_enabled` |
| `commands` | `cron` | 3 | `commands/cron.rs -> crate::cron::store::CronCreateParams` |
| `commands` | `process_util` | 3 | `commands/clipboard.rs -> crate::process_util::no_window_std` |
| `commands` | `update` | 3 | `commands/update.rs -> crate::update::checker::check_for_update` |
| `commands_mesh` | `config` | 3 | `commands_mesh.rs -> crate::config::ConfigState` |
| `crash` | `paths` | 3 | `crash/mod.rs -> crate::paths::data_dir` |
| `cron` | `config` | 3 | `cron/scheduler.rs -> crate::config::memory_config` |
| `dispatch` | `service` | 3 | `dispatch/mod.rs -> crate::service::ServiceContext` |
| `keepawake` | `api` | 3 | `keepawake.rs -> crate::api::events::emit_everywhere` |
| `keepawake` | `paths` | 3 | `keepawake.rs -> crate::paths::data_dir` |
| `lib` | `api` | 3 | `lib.rs -> crate::api::events::SystemEvent::ContributionsChanged` |
| `lib` | `secrets` | 3 | `lib.rs -> crate::secrets::migrate_native_bitwarden_to_plugin` |
| `lib` | `usage` | 3 | `lib.rs -> crate::usage::UsageStore::open` |
| `mesh` | `config` | 3 | `mesh/broadcast.rs -> crate::config::load_config` |
| `mesh` | `paths` | 3 | `mesh/manager.rs -> crate::paths::is_dev_instance` |
| `plugin` | `view` | 3 | `plugin/js_runtime/bubble.rs -> crate::view::Tone` |
| `pty` | `agent` | 3 | `pty/screen_state.rs -> crate::agent::native_prompt::NativePromptInput` |
| `secrets` | `paths` | 3 | `secrets/local.rs -> crate::paths::data_dir` |
| `service` | `provider` | 3 | `service/mod.rs -> crate::provider::ProviderRegistry` |
| `socket` | `agent_commands` | 3 | `socket/server/agent_commands.rs -> crate::agent_commands` |
| `socket` | `paths` | 3 | `socket/server/mod.rs -> crate::paths::socket_prefix` |
| `startup` | `process_util` | 3 | `startup/cli_install.rs -> crate::process_util::no_window_std` |
| `startup` | `session` | 3 | `startup/background.rs -> crate::session::SessionState` |
| `usage` | `agent` | 3 | `usage/probe.rs -> crate::agent::usage_parser::UsageParser` |
| `usage` | `service` | 3 | `usage/poller.rs -> crate::service::ServiceContext` |
| `webview2_reap` | `process_util` | 3 | `webview2_reap.rs -> crate::process_util::no_window_std` |
| `agent` | `api` | 2 | `agent/run_watcher.rs -> crate::api::events::emit_everywhere` |
| `agent` | `chatbackend` | 2 | `agent/provider/tests.rs -> crate::chatbackend::PresetInfo` |
| `agent` | `cron` | 2 | `agent/run_watcher.rs -> crate::cron::store::CronRunStatus::Success` |
| `agent` | `plugin` | 2 | `agent/provider.rs -> crate::plugin::SkillFile` |
| `api` | `logs` | 2 | `api/logs_routes.rs -> crate::logs::store::global` |
| `api` | `startup` | 2 | `api/bridge_routes.rs -> crate::startup::unmark_pane_connected_service` |
| `bridge` | `attachments` | 2 | `bridge/telegram.rs -> crate::attachments::evict` |
| `bridge` | `chatpipeline` | 2 | `bridge/voice_transcribe.rs -> crate::chatpipeline::run_voice_stage` |
| `commands` | `prompts` | 2 | `commands/pane.rs -> crate::prompts::PEER_ORCH_GREETING` |
| `commands` | `shell` | 2 | `commands/tests.rs -> crate::shell::ShellSyntax::NULL_DEV` |
| `config` | `paths` | 2 | `config/mod.rs -> crate::paths::config_path` |
| `config` | `service` | 2 | `config/schema/mod.rs -> crate::service::plan::reflection::DEFAULT_REFLECT_EVERY_GATES` |
| `cron` | `agent` | 2 | `cron/scheduler.rs -> crate::agent::models::canonicalize_short` |
| `cron` | `prompts` | 2 | `cron/scheduler.rs -> crate::prompts::CRON_REPORT_HINT` |
| `cron` | `service` | 2 | `cron/scheduler.rs -> crate::service::ServiceState` |
| `lib` | `agent` | 2 | `lib.rs -> crate::agent::provider::AgentProvider` |
| `lib` | `agent_commands` | 2 | `lib.rs -> crate::agent_commands::AgentCommandRegistryHandle` |
| `lib` | `bubble` | 2 | `lib.rs -> crate::bubble::BubbleRegistryHandle` |
| `lib` | `chatbackend` | 2 | `lib.rs -> crate::chatbackend::ChatBackendRegistryHandle` |
| `lib` | `chatpreprocessor` | 2 | `lib.rs -> crate::chatpreprocessor::ChatPreprocessorRegistryHandle` |
| `lib` | `config` | 2 | `lib.rs -> crate::config::save_config_pub` |
| `lib` | `crash` | 2 | `lib.rs -> crate::crash::check_previous_crashes` |
| `lib` | `delayed_send` | 2 | `lib.rs -> crate::delayed_send::DelayedSendQueue::open` |
| `lib` | `glance` | 2 | `lib.rs -> crate::glance::GlanceRegistryHandle` |
| `lib` | `logs` | 2 | `lib.rs -> crate::logs::LogStore::open` |
| `lib` | `plugin` | 2 | `lib.rs -> crate::plugin::system_plugins::SYSTEM_PLUGIN_HASHES` |
| `lib` | `view_host` | 2 | `lib.rs -> crate::view_host::ViewRegistryHandle` |
| `liveness` | `api` | 2 | `liveness.rs -> crate::api::events::emit_everywhere` |
| `mesh` | `api` | 2 | `mesh/toggle.rs -> crate::api::events::emit_everywhere` |
| `mesh` | `user_last_activity` | 2 | `mesh/manager.rs -> crate::user_last_activity::UserLastActivity` |
| `multiplexer` | `paths` | 2 | `multiplexer/state/workspace.rs -> crate::paths::normalize_cwd` |
| `plugin` | `config` | 2 | `plugin/js_runtime/chatbackend.rs -> crate::config::load_config` |
| `plugin` | `process_util` | 2 | `plugin/js_runtime/host_exec.rs -> crate::process_util::no_window_std` |
| `plugin` | `prompts` | 2 | `plugin/agent_claude/provider/integration.rs -> crate::prompts::CODETERM_RULE` |
| `prompts` | `service` | 2 | `prompts.rs -> crate::service::plan::reflection::ReflectionWindow` |
| `provider` | `chatbackend` | 2 | `provider/registry/tests.rs -> crate::chatbackend` |
| `runaway_sweeper` | `prompts` | 2 | `runaway_sweeper.rs -> crate::prompts::TESTING_DISCIPLINE` |
| `service` | `chatpreprocessor` | 2 | `service/mod.rs -> crate::chatpreprocessor::ChatPreprocessorRegistryHandle` |
| `service` | `delayed_send` | 2 | `service/mod.rs -> crate::delayed_send::DelayedSendQueue` |
| `service` | `logs` | 2 | `service/agent_restart.rs -> crate::logs::record` |
| `service` | `secrets` | 2 | `service/mod.rs -> crate::secrets::SecretStoreState` |
| `service` | `session` | 2 | `service/session_persist.rs -> crate::session::SessionState` |
| `session` | `multiplexer` | 2 | `session/mod.rs -> crate::multiplexer::commands::MuxState` |
| `socket` | `cron` | 2 | `socket/server/cron.rs -> crate::cron::store::CronCreateParams` |
| `startup` | `bridge` | 2 | `startup/bridge.rs -> crate::bridge` |
| `startup` | `mesh` | 2 | `startup/background.rs -> crate::mesh::types::PairingRequest` |
| `startup` | `notification` | 2 | `startup/states.rs -> crate::notification::store::NotificationStore` |
| `startup` | `pty` | 2 | `startup/states.rs -> crate::pty::manager` |
| `usage` | `config` | 2 | `usage/poller.rs -> crate::config::usage_tracking_enabled` |
| `usage` | `logs` | 2 | `usage/poller.rs -> crate::logs::record` |
| `view_host` | `plugin` | 2 | `view_host/mod.rs -> crate::plugin::capability::CapabilityRegistry` |
| `agent` | `delayed_send` | 1 | `agent/state_reactor.rs -> crate::delayed_send::create_message` |
| `agent` | `mem` | 1 | `agent/run_watcher.rs -> crate::mem::workers::save_worker_log` |
| `agent` | `paths` | 1 | `agent/model_catalog.rs -> crate::paths::data_dir` |
| `agent` | `plan` | 1 | `agent/reports.rs -> crate::plan::AttachmentRef` |
| `agent_commands` | `plugin` | 1 | `agent_commands/mod.rs -> crate::plugin::capability::CapabilityRegistry` |
| `api` | `chatpipeline` | 1 | `api/pane_routes/send.rs -> crate::chatpipeline::is_user_turn_send` |
| `api` | `pasteboard` | 1 | `api/pasteboard_routes.rs -> crate::pasteboard::PutParams` |
| `api` | `plugin` | 1 | `api/events.rs -> crate::plugin::activity::PluginActivity` |
| `api` | `resize_owner` | 1 | `api/pty_routes.rs -> crate::resize_owner::ResizeOwner` |
| `api` | `usage` | 1 | `api/usage_routes.rs -> crate::usage::projection::compute` |
| `api` | `view_host` | 1 | `api/web_plugin_routes.rs -> crate::view_host::plugin_ui_dir` |
| `bridge` | `agent` | 1 | `bridge/response.rs -> crate::agent::status::PaneStatus` |
| `bridge` | `startup` | 1 | `bridge/migrate_test.rs -> crate::startup::start_telegram_bridge_for_test` |
| `bubble` | `plugin` | 1 | `bubble/mod.rs -> crate::plugin::capability::CapabilityRegistry` |
| `bubble` | `view` | 1 | `bubble/mod.rs -> crate::view::Tone` |
| `chatbackend` | `plugin` | 1 | `chatbackend/mod.rs -> crate::plugin::capability::CapabilityRegistry` |
| `chatbackend` | `service` | 1 | `chatbackend/mod.rs -> crate::service::pane_transport` |
| `chatpreprocessor` | `chatpipeline` | 1 | `chatpreprocessor/mod.rs -> crate::chatpipeline` |
| `chatpreprocessor` | `plugin` | 1 | `chatpreprocessor/mod.rs -> crate::plugin::capability::CapabilityRegistry` |
| `commands` | `bridge` | 1 | `commands/pane.rs -> crate::bridge::voice_transcribe::format_attachment_line` |
| `commands` | `bubble` | 1 | `commands/plugins/view.rs -> crate::bubble::BubbleRegistry` |
| `commands` | `chatpipeline` | 1 | `commands/pane.rs -> crate::chatpipeline::is_user_turn_send` |
| `commands` | `glance` | 1 | `commands/plugins/view.rs -> crate::glance::GlanceRegistry` |
| `commands` | `resize_owner` | 1 | `commands/pty.rs -> crate::resize_owner::ResizeOwner` |
| `commands` | `socket` | 1 | `commands/pty.rs -> crate::socket::server::socket_display_path` |
| `commands` | `usage` | 1 | `commands/usage.rs -> crate::usage::projection::compute` |
| `commands` | `view_host` | 1 | `commands/plugins/view.rs -> crate::view_host::ViewRegistry` |
| `config` | `api` | 1 | `config/mod.rs -> crate::api::events::emit_everywhere` |
| `config` | `mesh` | 1 | `config/mod.rs -> crate::mesh::broadcast_announce` |
| `config` | `update` | 1 | `config/schema/mod.rs -> crate::update::Channel` |
| `crash` | `keepawake` | 1 | `crash/panic.rs -> crate::keepawake::cleanup_active_children_for_exit` |
| `cron` | `paths` | 1 | `cron/store/mod.rs -> crate::paths::data_dir` |
| `delayed_send` | `api` | 1 | `delayed_send.rs -> crate::api::events::emit_everywhere` |
| `dispatch` | `api` | 1 | `dispatch/mod.rs -> crate::api::agent_routes::list_agent_provider_values` |
| `glance` | `plugin` | 1 | `glance/mod.rs -> crate::plugin::capability::CapabilityRegistry` |
| `glance` | `view` | 1 | `glance/mod.rs -> crate::view::GlanceView` |
| `keepawake` | `process_util` | 1 | `keepawake.rs -> crate::process_util::no_window_std` |
| `lib` | `commands` | 1 | `lib.rs -> crate::commands::mirror_skills_to_providers` |
| `lib` | `liveness` | 1 | `lib.rs -> crate::liveness::start_api_liveness_watchdog` |
| `lib` | `pasteboard` | 1 | `lib.rs -> crate::pasteboard::PasteboardState::new` |
| `lib` | `provider` | 1 | `lib.rs -> crate::provider::ProviderRegistry::new` |
| `lib` | `transcribe` | 1 | `lib.rs -> crate::transcribe::TranscribeRegistry::new` |
| `lib` | `user_last_activity` | 1 | `lib.rs -> crate::user_last_activity::UserLastActivity::new` |
| `liveness` | `process_util` | 1 | `liveness.rs -> crate::process_util::no_window_std` |
| `logs` | `api` | 1 | `logs/store.rs -> crate::api::events::emit_everywhere` |
| `mem` | `api` | 1 | `mem/workers.rs -> crate::api::events::emit_everywhere` |
| `mem` | `paths` | 1 | `mem/workers.rs -> crate::paths::data_dir` |
| `mem` | `pty` | 1 | `mem/workers.rs -> crate::pty::ansi::clean_terminal_output_sized` |
| `multiplexer` | `lock_hold` | 1 | `multiplexer/commands.rs -> crate::lock_hold::TimedRwLock` |
| `plugin` | `shell` | 1 | `plugin/agent_claude/provider/launch.rs -> crate::shell::ShellSyntax` |
| `plugin` | `socket` | 1 | `plugin/js_runtime/chatbackend.rs -> crate::socket::server::socket_display_path` |
| `pty` | `paths` | 1 | `pty/shell_integration.rs -> crate::paths::data_dir` |
| `pty` | `process_util` | 1 | `pty/manager.rs -> crate::process_util::no_window_std` |
| `runaway_sweeper` | `config` | 1 | `runaway_sweeper.rs -> crate::config::schema::RunawayConfig` |
| `runaway_sweeper` | `multiplexer` | 1 | `runaway_sweeper.rs -> crate::multiplexer::state::MultiplexerState` |
| `runaway_sweeper` | `service` | 1 | `runaway_sweeper.rs -> crate::service::ServiceContext` |
| `secrets` | `api` | 1 | `secrets/api.rs -> crate::api::routes` |
| `secrets` | `plugin` | 1 | `secrets/mod.rs -> crate::plugin::capability::CapabilityRegistry` |
| `secrets` | `process_util` | 1 | `secrets/secret_file.rs -> crate::process_util::no_window_std` |
| `service` | `agent_commands` | 1 | `service/mod.rs -> crate::agent_commands::AgentCommandRegistryHandle` |
| `service` | `bubble` | 1 | `service/mod.rs -> crate::bubble::BubbleRegistryHandle` |
| `service` | `chatpipeline` | 1 | `service/pane_transport.rs -> crate::chatpipeline::run_text_stages` |
| `service` | `glance` | 1 | `service/mod.rs -> crate::glance::GlanceRegistryHandle` |
| `service` | `notification` | 1 | `service/notification.rs -> crate::notification::store::NotificationLevel` |
| `service` | `pasteboard` | 1 | `service/mod.rs -> crate::pasteboard::PasteboardState` |
| `service` | `shell` | 1 | `service/orchestrator/tests.rs -> crate::shell::ShellSyntax` |
| `service` | `transcribe` | 1 | `service/mod.rs -> crate::transcribe::TranscribeRegistry` |
| `service` | `user_last_activity` | 1 | `service/mod.rs -> crate::user_last_activity::UserLastActivity` |
| `service` | `view_host` | 1 | `service/mod.rs -> crate::view_host::ViewRegistryHandle` |
| `session` | `paths` | 1 | `session/store.rs -> crate::paths::data_dir` |
| `socket` | `crash` | 1 | `socket/server/mod.rs -> crate::crash::trigger_test_crash` |
| `socket` | `mem` | 1 | `socket/server/plan_read.rs -> crate::mem::search::SearchQuery` |
| `socket` | `prompts` | 1 | `socket/server/plan.rs -> crate::prompts::peer_pairing_mandate_full` |
| `startup` | `lock_hold` | 1 | `startup/states.rs -> crate::lock_hold` |
| `startup` | `resize_owner` | 1 | `startup/states.rs -> crate::resize_owner::ResizeOwnerRegistry::default` |
| `startup` | `runaway_sweeper` | 1 | `startup/background.rs -> crate::runaway_sweeper::sweep_once` |
| `transcribe` | `plugin` | 1 | `transcribe/mod.rs -> crate::plugin::capability::CapabilityRegistry` |
| `update` | `agent` | 1 | `update/shutdown.rs -> crate::agent::provider::AgentProvider` |
| `update` | `crash` | 1 | `update/shutdown.rs -> crate::crash::shutdown::mark_shutting_down` |
| `update` | `mem` | 1 | `update/shutdown.rs -> crate::mem::MemManager::open_in_memory` |
| `update` | `paths` | 1 | `update/post_update.rs -> crate::paths::data_dir` |
| `update` | `plugin` | 1 | `update/shutdown.rs -> crate::plugin::agent_claude::ClaudeProvider` |
| `update` | `startup` | 1 | `update/apply.rs -> crate::startup::refresh_bundled_cli` |
| `usage` | `api` | 1 | `usage/poller.rs -> crate::api::events::emit_everywhere` |
| `usage` | `plugin` | 1 | `usage/probe.rs -> crate::plugin::agent_claude::usage_tui::ClaudeTuiUsageParser` |
| `user_last_activity` | `paths` | 1 | `user_last_activity.rs -> crate::paths::is_dev_instance` |
| `webview2_reap` | `crash` | 1 | `webview2_reap.rs -> crate::crash::shutdown::clean_exit_timestamp` |
| `webview2_reap` | `paths` | 1 | `webview2_reap.rs -> crate::paths::data_dir` |
