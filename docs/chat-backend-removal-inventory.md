# Core chat backend removal inventory

Decision packet only. Slice 6 retires the external plugin; core transport removal is deferred. No change to `service/chat_send_transaction.rs`, `service/pty_io/`, the PTY write queue or delivery ledger belongs to this slice.

## Ownership and rollback

The later removal must land atomically across persistence, provider capabilities, dispatch, RPC, frontend and tests. Preserve `TabBacking::ChatBackend { plugin_id, session_id }` decoding until restored tabs can be represented by an explicit removed-provider state. Never coerce those tabs to `TabBacking::Pty`: that would spawn an unrelated shell and lose the session identity. Preserve tab ids, hierarchy links and saved history. Archived Router branches in the plugins repository remain evidence and are not deleted.

Rollback boundary: before retiring core, revert the channel-retirement commit and reinstall the archived plugin if required. Once core removal lands, rollback requires restoring the complete capability/transport/serialization boundary together, then installing the plugin. Never replay pending input or infer readiness from elapsed time.

## Removal seams

- Contract and registration: `chatbackend::ChatBackendProvider`, `ChatBackendRegistry`, `ChatBackendError`; JS `JsChatBackend`; SDK `ChatBackendCapability`; manifest `capabilities.chatBackend`; capability descriptors and provider registration. Keep the independent `decisionModel` and `chatPreprocessor` capabilities.
- Persistence: `codeterm-pty/src/multiplexer/types.rs` defines `TabBacking`; session snapshots serialize it. Restore/snapshot tests must cover a removed backend without opening a PTY or changing tab identity.
- Spawn/restore: `spawn_chatbackend_impl_with_state_change`, `chatbackend_launch`, `spawn_chatbackend_poll_loop`, `ChatBackendTransport`, `chatbackend_reopen_transition`, `chatbackend_reopen_still_valid`, `chatbackend_reopen_if_restored` and `TabReopenGuards`. Close owned plugin sessions once before unregistering; do not dispose unrelated PTYs.
- Dispatch: `TabTransport`, transport resolution, spawn parameters, agent registries, watcher ticks, messenger and planner delivery. The `chat_send_transaction` branch is protected: its removal needs the separate PTY-delivery skill and explicit owner authorization with at-most-once/uncertainty regression proof.
- History/context: `tab_chat::chatbackend_history`, `chatbackend_transcript_snapshot`, `chatbackend_snapshot_from`, host context composition and history migrations. Decide retention separately from transport deletion; archived transcripts remain user data.
- RPC/CLI: `open_chat_backend_tab`, `chatbackend_session_info`, `chatbackend_switch_model`, `chat_start_core`, service transport commands and CLI tab/agent model/preset verbs. Regenerate command contracts from canonical; do not hand edit generated CLI outputs.
- Frontend: `TabBacking` types, bridge mappings, transport-aware provider lookup, `TerminalTab`, `TabSessionChat`, model/preset bubbles, open-shell context actions and missing-plugin state. PTY transcript chat and capability-declared structured prompts remain independent.
- Tests: shared native/JS chatbackend parity, context ownership, service spawn/reopen/concurrency, restored snapshots, CLI labels/team utility, frontend provider/model/preset/removed-plugin tests. Delete only tests whose contract is removed; retain PTY/session ownership invariants and independent decision-model parity.

## Mechanical file inventory

Tracked references at the slice-6 lane head plus its source overlay. Paths below include tests and consumers, not an instruction to delete every file. Re-run `git grep -l -I -E 'chatBackend|chat_backend|ChatBackend' -- src-tauri src packages cli prod-docs essences` on canonical before implementation because other lanes can add consumers.

### Native application

- `src-tauri/src/agent/live_state.rs`
- `src-tauri/src/agent/model_binary_watch.rs`
- `src-tauri/src/agent/provider/tests.rs`
- `src-tauri/src/api/agent_model_catalog_routes.rs`
- `src-tauri/src/api/agent_routes/tests.rs`
- `src-tauri/src/api/routes.rs`
- `src-tauri/src/api/tab_chat_routes.rs`
- `src-tauri/src/api/tab_routes.rs`
- `src-tauri/src/api/tab_routes/send.rs`
- `src-tauri/src/api/tab_routes/tests_create_tab.rs`
- `src-tauri/src/api/tab_routes/tests_send_text_body.rs`
- `src-tauri/src/api/web_routes.rs`
- `src-tauri/src/commands/agent.rs`
- `src-tauri/src/commands/plugins/snapshot.rs`
- `src-tauri/src/commands/tab.rs`
- `src-tauri/src/cron/recovery/tests.rs`
- `src-tauri/src/cron/scheduler.rs`
- `src-tauri/src/cron/spawn_spec.rs`
- `src-tauri/src/cron/spawn_spec/tests.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/mem_host/worker_spawn_spec.rs`
- `src-tauri/src/mem_host/worker_spawn_spec/tests.rs`
- `src-tauri/src/prompts.rs`
- `src-tauri/src/provider/registry.rs`
- `src-tauri/src/provider/registry/tests.rs`
- `src-tauri/src/service/agent_restart/exit_recovery.rs`
- `src-tauri/src/service/hierarchy/heartbeat_gate/snapshot.rs`
- `src-tauri/src/service/hierarchy/heartbeat_gate/snapshot_tests.rs`
- `src-tauri/src/service/hierarchy/planner_delivery.rs`
- `src-tauri/src/service/messenger.rs`
- `src-tauri/src/service/messenger/tests.rs`
- `src-tauri/src/service/mod.rs`
- `src-tauri/src/service/orchestration_pair/tests.rs`
- `src-tauri/src/service/orchestrator/launch/tests.rs`
- `src-tauri/src/service/orchestrator/spawn.rs`
- `src-tauri/src/service/orchestrator_onboard.rs`
- `src-tauri/src/service/plan_reconcile.rs`
- `src-tauri/src/service/pty_io/lifecycle.rs`
- `src-tauri/src/service/pty_io/send/mod.rs`
- `src-tauri/src/service/receiver_target.rs`
- `src-tauri/src/service/shell_restart.rs`
- `src-tauri/src/service/spawn.rs`
- `src-tauri/src/service/spawn_capability/test_backend.rs`
- `src-tauri/src/service/spawn_capability/tests.rs`
- `src-tauri/src/service/spawn_chatbackend.rs`
- `src-tauri/src/service/spawn_params.rs`
- `src-tauri/src/service/spawn_params/transport_tests.rs`
- `src-tauri/src/service/spawn_tests.rs`
- `src-tauri/src/service/spawn_tests_chatbackend.rs`
- `src-tauri/src/service/supporter_tab_state.rs`
- `src-tauri/src/service/supporter_tab_state/metadata_tests.rs`
- `src-tauri/src/service/supporter_tab_state/tests.rs`
- `src-tauri/src/service/tab_chat/chatbackend_history.rs`
- `src-tauri/src/service/tab_chat/context_window.rs`
- `src-tauri/src/service/tab_chat/host_context.rs`
- `src-tauri/src/service/tab_chat/host_context/tests.rs`
- `src-tauri/src/service/tab_chat/mod.rs`
- `src-tauri/src/service/tab_chat/structured_sink.rs`
- `src-tauri/src/service/tab_chat/tests.rs`
- `src-tauri/src/service/tab_lifecycle.rs`
- `src-tauri/src/service/tab_transport.rs`
- `src-tauri/src/service/tab_transport_chat.rs`
- `src-tauri/src/service/tab_transport_chat/tests/context_owner.rs`
- `src-tauri/src/service/team/close.rs`
- `src-tauri/src/service/team/tests/close_lifecycle.rs`
- `src-tauri/src/service/test_fixture.rs`
- `src-tauri/src/service/transport.rs`
- `src-tauri/src/service/transport_tests.rs`
- `src-tauri/src/service/turn_resume/tests.rs`
- `src-tauri/src/service/workspace.rs`
- `src-tauri/src/service/workspace/workspace_close_tests.rs`
- `src-tauri/src/socket/server/tab.rs`
- `src-tauri/src/startup/servers.rs`
- `src-tauri/src/startup/states.rs`

### Shared native crates

- `src-tauri/crates/codeterm-plan/src/service_plan/planner_requests.rs`
- `src-tauri/crates/codeterm-plugin/src/chatbackend_provider.rs`
- `src-tauri/crates/codeterm-plugin/src/js_runtime/chatbackend.rs`
- `src-tauri/crates/codeterm-plugin/src/js_runtime/hot_reload.rs`
- `src-tauri/crates/codeterm-plugin/src/js_runtime/hot_reload/tests.rs`
- `src-tauri/crates/codeterm-plugin/src/js_runtime/manifest.rs`
- `src-tauri/crates/codeterm-plugin/src/js_runtime/manifest/capabilities.rs`
- `src-tauri/crates/codeterm-plugin/src/js_runtime/manifest/tests.rs`
- `src-tauri/crates/codeterm-plugin/src/js_runtime/mod.rs`
- `src-tauri/crates/codeterm-plugin/src/js_runtime/parity_test.rs`
- `src-tauri/crates/codeterm-plugin/src/js_runtime/parity_test/context_owner.rs`
- `src-tauri/crates/codeterm-plugin/src/js_runtime/vm.rs`
- `src-tauri/crates/codeterm-plugin/src/provider.rs`
- `src-tauri/crates/codeterm-plugin/src/provider/capabilities.rs`
- `src-tauri/crates/codeterm-pty/src/dispatch/mod.rs`
- `src-tauri/crates/codeterm-pty/src/multiplexer/state/closed/mod.rs`
- `src-tauri/crates/codeterm-pty/src/multiplexer/state/closed/store.rs`
- `src-tauri/crates/codeterm-pty/src/multiplexer/state/closed/tests.rs`
- `src-tauri/crates/codeterm-pty/src/multiplexer/state/snapshot/mod.rs`
- `src-tauri/crates/codeterm-pty/src/multiplexer/state/snapshot/tests.rs`
- `src-tauri/crates/codeterm-pty/src/multiplexer/state/tests.rs`
- `src-tauri/crates/codeterm-pty/src/multiplexer/types.rs`
- `src-tauri/crates/codeterm-pty/src/pty/scrollback_capture.rs`
- `src-tauri/crates/codeterm-pty/src/session/lifecycle/tests.rs`
- `src-tauri/crates/codeterm-pty/src/session/store.rs`
- `src-tauri/crates/codeterm-types/src/plan/types.rs`
- `src-tauri/crates/codeterm-types/src/pty/agent_liveness.rs`

### Frontend

- `src/App.tsx`
- `src/components/AgentPicker/preselectProvider.test.ts`
- `src/components/Automation/AutomationGraphView.test.tsx`
- `src/components/Automation/fixtures.testdata.ts`
- `src/components/Canvas/canvasApi.test.ts`
- `src/components/Canvas/canvasApi.ts`
- `src/components/Canvas/planView/PlanAskExchange.tsx`
- `src/components/Canvas/planView/PlanCompanionBubble.test.tsx`
- `src/components/Canvas/planView/PlanCompanionBubble.tsx`
- `src/components/Canvas/planView/planAskModel.ts`
- `src/components/Extensions/CapabilityChip.tsx`
- `src/components/Extensions/ChatModelBubble.test.tsx`
- `src/components/Extensions/ChatModelBubble.tsx`
- `src/components/Extensions/ChatPresetBubble.tsx`
- `src/components/Extensions/ConfirmDialog.tsx`
- `src/components/Extensions/PluginDetailPanel.tsx`
- `src/components/Extensions/PresetManager.test.tsx`
- `src/components/Extensions/PresetManager.tsx`
- `src/components/Extensions/chatModelBubbleApi.ts`
- `src/components/Extensions/presetManagerApi.ts`
- `src/components/Extensions/types.ts`
- `src/components/Sidebar/SidebarWorkspaceTabs.staffing.test.tsx`
- `src/components/Sidebar/SidebarWorkspaceTabs.test.tsx`
- `src/components/Sidebar/TabItem.tsx`
- `src/components/Sidebar/TeamUnit.tsx`
- `src/components/Sidebar/orchestratorRecoveryMenu.test.ts`
- `src/components/Sidebar/orchestratorRecoveryMenu.ts`
- `src/components/Sidebar/tabModelMenu.test.ts`
- `src/components/Sidebar/tabModelMenu.ts`
- `src/components/Sidebar/tabProviderMenu.test.ts`
- `src/components/Sidebar/tabProviderMenu.ts`
- `src/components/Terminal/ChatComposer.tsx`
- `src/components/Terminal/TabContent.tsx`
- `src/components/Terminal/TabModeSwitch.tsx`
- `src/components/Terminal/TabSessionChat.delivery.test.tsx`
- `src/components/Terminal/TabSessionChat.origin-actions.test.tsx`
- `src/components/Terminal/TabSessionChat.origin-stability.test.tsx`
- `src/components/Terminal/TabSessionChat.planref-stability.test.tsx`
- `src/components/Terminal/TabSessionChat.tsx`
- `src/components/Terminal/TabSessionChat.watcher.test.tsx`
- `src/components/Terminal/TerminalTab.tsx`
- `src/components/Terminal/tabSessionActions.test.ts`
- `src/components/Terminal/tabSessionActions.ts`
- `src/components/Terminal/useChatSession.droppedDelta.test.tsx`
- `src/components/Terminal/useChatSession.ts`
- `src/lib/agentDescriptorCache.test.ts`
- `src/lib/agentLabel.test.ts`
- `src/lib/agentLabel.ts`
- `src/lib/bridge.ts`
- `src/lib/chatUiMode.ts`
- `src/lib/orchestratorOnboard.test.ts`
- `src/lib/orchestratorOnboardEligibility.ts`
- `src/lib/remoteTabPortal.test.ts`
- `src/lib/tabProvider.test.ts`
- `src/lib/tabProvider.ts`
- `src/lib/types.ts`
- `src/lib/useTabProvider.ts`
- `src/store/multiplexer/muxSelectorPerf.test.tsx`

### SDK

- `packages/chat-engine/package.json`
- `packages/chat-ui/src/MessageBubble.toolcards.test.tsx`
- `packages/plugin-sdk/src/capabilities.ts`
- `packages/plugin-sdk/src/host.ts`
- `packages/plugin-sdk/src/types.test.ts`

### CLI

- `cli/src/args/agent.rs`
- `cli/src/args/misc.rs`
- `cli/src/cmd/plugin/validate.rs`
- `cli/src/cmd/send_text.rs`
- `cli/src/cmd/send_text/tests.rs`
- `cli/src/cmd/tab.rs`
- `cli/src/cmd/tab/team_utility.rs`
- `cli/src/mesh_cli/api_map.rs`
- `cli/src/mesh_cli/tests.rs`
- `cli/src/subcommand_contract_tests.rs`

### Contracts and docs

- `prod-docs/API-AGENTS.md`
- `prod-docs/API-CLI.md`
- `prod-docs/API.md`
- `prod-docs/TAB-LIFECYCLE.md`
