# COD-772 oracle disposition

Source audit base: `030cd15f8d9da9a289cd01ac03d53c3c023f6a74`. This is a source-backed retirement ledger, not executed test evidence. All runtime/platform observations remain UNVERIFIED until canonical receipts name the executing SHA.

The provider-specific WebSocket sidecar, listener/token files, remote TUI attachment and retry-to-terminal fallback are retired. Runtime selection uses `agentRuntime`; explicit PTY backing keeps the PTY transaction and own-session append-window evidence. Runtime mode declarations move to `agentRuntime.runtimeModes`. Generic role/task/bootstrap, compaction, exact session/nonce/epoch and no-replay boundaries remain.

Retained paths below are relative to `src-tauri/crates/codeterm-service-core/src/service/agent_runtime/` unless prefixed `plugins/` or otherwise stated. A retained reference describes source coverage, never a fresh PASS.

## Deleted module test ledger

### `approvals/tests.rs`

Disposition: retained by `model/tests.rs; subscriber/tests.rs; prompt_answers.rs; plugins/provider-codex/tests/runtime.test.ts`. Exact request ids, bounded outstanding requests, option identity and fail-closed unsupported requests live in the generic owner/adapter.

- `codex_rpc_control_approval_reply_mapping`
- `codex_rpc_control_user_input_collects_each_question_and_free_text`
- `codex_rpc_control_elicitation_and_dynamic_tools_fail_closed`
- `codex_rpc_control_resolved_clears_only_exact_request_and_promotes_next`
- `codex_rpc_control_ui_option_routes_and_numeric_free_text_round_trip`

### `attachment.rs`

Disposition: retired transport mechanism. Own initialize/resume attachment handshake is obsolete: no remote TUI or listener remains. Generic initialize/launch authority uses engine/launch/tests.rs and model/tests.rs.

- `codex_rpc_attach_requires_own_initialize_and_bound_resume_success`
- `codex_rpc_attach_rejects_failed_or_unmatched_protocol_replies`

### `auth.rs`

Disposition: retained by `login/tests.rs; engine/tests/sign_in.rs; plugins/provider-codex/tests/access-errors.test.ts`. Account state, failed/unknown credentials and scoped login remain adapter events; no credential host boundary changes.

- `codex_rpc_access_errors_are_structural_and_clear_only_on_success`
- `codex_rpc_auth_account_read_has_no_guessed_login_state`

### `bootstrap.rs`

Disposition: retained by `engine/tests/bootstrap_ownership.rs; engine/tests/bootstrap_ownership/overlap.rs`. The generic bootstrap owner proves sibling isolation, at-most-once claims and exact send identity.

- `codex_rpc_review_bootstrap_consumes_only_own_client_id`

### `buffer.rs`

Disposition: retained by `rpc/tests.rs; events/tests.rs; engine/backfill/tests.rs`. Generic bounded queues and history generation/cursors replace the WebSocket prebind buffer; its root-ready attachment latch is retired.

- `codex_rpc_buffer_preserves_order_and_fails_closed_on_overflow`
- `codex_rpc_control_buffer_keeps_root_ready_and_retains_prebind_requests`

### `capability.rs`

Disposition: retired transport mechanism. Provider-specific protocol/version/config toggle is removed. agent_runtime capability selects transport; generic modes parity lives in codeterm-plugin/src/js_runtime/parity_test/runtime_modes.rs. Provider PTY resume version checks stay in plugins/provider-codex/tests/launch-spec.test.ts.

- `codex_rpc_capability_defaults_on_and_checks_protocol`
- `codex_rpc_version_gate`

### `client/tests.rs`

Disposition: retained by `rpc/tests.rs; engine/backfill/tests.rs; transport/tests.rs; plugins/provider-codex/tests/runtime-history-pager.test.ts`. Framing, correlation, EOF, bounded memory, history paging and no replay retain generic coverage. The old global-config allowlist is replaced by generic operation method allowlists and provider per-thread plans.

- `codex_rpc_client_connection_failure_distinguishes_transport_from_refusal`
- `codex_rpc_client_keeps_server_ids_separate`
- `codex_rpc_client_initialize_once_then_notification`
- `codex_rpc_client_death_fails_pending_and_rejects_new_requests`
- `codex_rpc_client_forbids_global_config_writes`
- `codex_rpc_control_history_paginates_turns_and_items_without_jsonl`
- `codex_rpc_control_history_optional_goal_never_discards_transcript`
- `codex_rpc_control_turn_requests_cache_context_by_exact_thread_and_reply_ids`
- `codex_rpc_control_buffer_overflow_requests_resync_without_closing_transport`
- `codex_rpc_control_retired_socket_cannot_close_new_connection`
- `codex_rpc_pending_steer_interrupt_drops_server_buffer_without_replay`

### `compaction.rs`

Disposition: retained by `subscriber/compaction_tests.rs; plugins/provider-codex/tests/runtime.test.ts`. Role/task/additional context restore once per compaction and Compacting state remain generic oracles.

- `codex_rpc_control_developer_contract_is_composed_without_task_loss`
- `codex_rpc_control_compaction_restores_contract_and_context_once_per_item`

### `control.rs`

Disposition: retained by `controls/interrupt/tests.rs; subscriber/turn_lifecycle_tests.rs; plugins/provider-codex/tests/runtime-interrupts.test.ts`. Exact turn/descendant interruption and old-turn protection remain in the generic owner.

- `codex_rpc_control_restore_does_not_replay_stop_or_settle_a_newer_turn`
- `codex_rpc_control_stop_deduplicates_and_interrupts_descendants`

### `delivery.rs`

Disposition: retired transport mechanism. Two-latch remote TUI attachment/reconnect delivery state is removed. Generic runtime bootstrap uses exact bound thread and send claim (engine/tests/bootstrap_ownership.rs); PTY readiness stays in orchestrator/launch/user_input.rs and public_send_readiness.

- `codex_rpc_send_plain_fallback_is_terminal_without_rpc_attach_or_bootstrap`
- `codex_rpc_attach_delivery_waits_for_both_latches_and_wakes_once`
- `codex_rpc_attach_failure_keeps_pending_after_endpoint_clear`
- `codex_rpc_attach_overlapping_connections_keep_ready_until_last_closes`
- `codex_rpc_attach_short_and_duplicate_connections_cannot_demote_qualified_attachment`
- `codex_rpc_attach_many_overlapping_connections_preserve_existing_authority`
- `codex_rpc_attach_recovery_and_late_failure_preserve_reconnected_live_client`

### `launch.rs`

Disposition: retired transport mechanism. Remote-TUI command/cwd/token env and deferred sidecar preparation are removed. Existing generic launch_options/tests.rs, engine/launch/tests.rs and PTY direct_command/launch tests retain model/effort/resume/cwd/environment authority.

- `codex_rpc_launch_spawn_origins_use_resolved_cwd`
- `codex_rpc_launch_without_cwd_pins_daemon_directory`
- `codex_rpc_launch_reproduces_os_error_123_and_resolves_long_prompt_cwd`
- `codex_rpc_launch_normalizes_verbatim_workspace_before_deferral`
- `codex_rpc_launch_uses_absolute_binary_and_token_env_name_only`
- `codex_rpc_control_hidden_selection_preserves_model_effort_and_resume`

### `lifecycle.rs`

Disposition: retired transport mechanism. WebSocket sidecar reuse, private listener/token file and archived-error resume retries are removed. Generic process_slot/tests.rs retains generation/cleanup proof; provider runtime.test.ts retains archived conversation handling.

- `codex_rpc_control_concurrent_recovery_reuses_live_server_without_replacement`
- `codex_rpc_lifecycle_tokens_are_tab_scoped_and_file_is_exclusive`
- `codex_rpc_lifecycle_failed_start_removes_private_artifacts`
- `codex_rpc_restore_unarchive_matches_only_archived_errors`

### `model/tests.rs`

Disposition: retained by `controls/tests.rs; restore/tests.rs; plugins/provider-codex/tests/runtimeSettings.test.ts`. Runtime modes, model/effort settings and live confirmation retain generic coverage. Next-turn TUI sidecar settings are retired; generic runtime uses confirmed live settings.

- `codex_rpc_settings_need_a_subsequent_own_turn_and_requested_effort`
- `codex_rpc_next_turn_request_uses_shared_pending_observation`
- `codex_rpc_control_runtime_modes_and_next_turn_overrides`
- `codex_rpc_recorded_settings_reconfirm_restored_effort_for_current_binding`
- `codex_rpc_effort_settings_distinguish_absent_unknown_and_unset`

### `normalize/recovery_tests.rs`

Disposition: retained by `model/tests.rs; engine/backfill/tests.rs; plugins/provider-codex/tests/runtime.test.ts`. Protocol normalization is provider-owned. Existing recorded replay fixtures cover item ids/order/live/history, deltas, tools, MCP, plans, errors and compaction. UI mixed-turn fixture moved intact to src/components/AgentRun/fixtures/mixed-turn.json.

- `restored_active_schedules_replace_from_item_completion_without_replay`
- `restored_active_schedules_replace_from_interrupted_turn_history_without_completion`
- `restored_turn_read_failure_retains_snapshot_and_never_requests_again`
- `closed_restored_item_never_requests_history_on_turn_completion`

### `normalize/tests.rs`

Disposition: retained by `model/tests.rs; engine/backfill/tests.rs; plugins/provider-codex/tests/runtime.test.ts`. Protocol normalization is provider-owned. Existing recorded replay fixtures cover item ids/order/live/history, deltas, tools, MCP, plans, errors and compaction. UI mixed-turn fixture moved intact to src/components/AgentRun/fixtures/mixed-turn.json.

- `codex_rpc_normalize_split_deltas_preserve_exact_line_structure`
- `codex_rpc_control_restored_items_keep_order_and_do_not_replay_old_deltas`
- `codex_rpc_normalize_recorded_simple_keeps_app_server_item_id`
- `codex_rpc_normalize_recorded_command_preserves_outcome`
- `codex_rpc_normalize_recorded_file_changes_and_reasoning`
- `codex_rpc_mixed_turn_live_and_history_match_chat_wire_contract`
- `codex_rpc_all_schema_items_have_live_and_history_models`
- `codex_rpc_mcp_results_errors_progress_and_arguments_keep_exact_content`
- `codex_rpc_user_images_compaction_plans_and_turn_errors_are_preserved`
- `codex_rpc_rich_tools_diffs_and_agent_receivers_preserve_details`
- `codex_rpc_normalize_deltas_accumulate_without_changing_identity`
- `codex_rpc_normalize_ignores_unknown_events_and_isolates_threads`
- `codex_rpc_command_batch_identity_matches_live_and_history`

### `ownership.rs`

Disposition: retained by `ownership.rs`. Byte-identical lease_read oracle remains in the generic runtime; no ownership semantics change.

- `ownership_requires_a_live_lease`

### `process.rs`

Disposition: retained by `process/tests.rs; process_boundary/tests.rs; process_slot/tests.rs`. Generic child environment, startup/exit evidence, process ownership and cleanup remain. Private listener/token-file sweep cases are redundant because the retired path no longer creates those artifacts.

- `codex_rpc_process_exit_observer_interrupt_is_not_exit`
- `codex_rpc_process_startup_exit_retains_status_and_stderr`
- `codex_rpc_process_empty_cwd_error_names_exact_invalid_path`
- `codex_rpc_process_sweep_leaves_foreign_instance_artifacts`
- `codex_rpc_process_scrubs_provider_identity_through_shared_child_env`
- `codex_rpc_process_cleanup_waits_for_exit_evidence`
- `codex_rpc_process_sweep_removes_stale_owned_artifacts`

### `process_output.rs`

Disposition: retained by `process/tests.rs`. Bounded stderr and process exit status survive in generic process coverage. Listening-string readiness is retired with the listener.

- `startup_output_retains_bounded_latest_stderr`
- `startup_output_reports_exit_and_preserves_invalid_utf8_evidence`
- `startup_output_never_invents_exit_evidence`
- `startup_output_detects_listening_across_read_boundaries`

### `protocol/tests.rs`

Disposition: retained by `rpc/tests.rs; plugins/provider-codex/tests/runtime.test.ts`. Generic JSON-RPC correlation/framing and provider initialize/unsupported-method plans retain the contract.

- `codex_rpc_framing_without_jsonrpc_and_unknown_fields`
- `codex_rpc_server_request_id_does_not_become_response`
- `codex_rpc_rejects_ambiguous_or_malformed_responses`
- `codex_rpc_missing_method_is_unavailable`
- `codex_rpc_initialize_declares_experimental_and_opt_out`

### `recovery.rs`

Disposition: retired transport mechanism. Remote-TUI reattachment/reconnect is removed. Generic runtime never falls back to a terminal; retry resumes exact thread through engine binding validation.

- `codex_rpc_attach_failure_retains_server_and_reattaches_current_launch_once`
- `codex_rpc_attach_live_connection_wins_over_failed_recovery`
- `codex_rpc_attach_current_reconnect_removes_failed_recovery_and_stale_nonce_does_not`
- `codex_rpc_attach_live_or_unknown_provider_waits_for_protocol_without_shell_write`

### `routing/tests.rs`

Disposition: retained by `children/tests.rs; engine/tests/sync_send.rs; plugins/provider-codex/tests/runtime.test.ts`. Exact own-thread authority and authenticated descendant read-only scope remain generic.

- `codex_rpc_thread_read_rejects_children_and_unknown_sources`

### `schema.rs`

Disposition: retained by `rpc/tests.rs; plugins/provider-codex/tests/runtime.test.ts`. Provider protocol fields remain provider-owned JSON; generic framing/recorded reductions preserve forward-compatible unknown-field handling.

- `codex_rpc_schema_ignores_new_fields`

### `send/gate.rs`

Disposition: retained by `transport/tests.rs; engine/tests/sync_send.rs; engine/tests/rejected_before_write.rs; engine/tests/bootstrap_ownership.rs`. Generic send owner retains one write, exact claim/client evidence, busy inputs, refusal/uncertainty, settings and no PTY fallback. TUI-attachment delivery-state gates are retired.

- `codex_rpc_send_before_readiness_requires_durable_admission`
- `codex_rpc_send_post_ready_attachment_loss_does_not_gate_verified_session_input`
- `codex_rpc_send_requires_available_endpoint_and_own_binding`
- `codex_rpc_send_plain_recovery_and_capability_off_restore_pty`

### `send/startup.rs`

Disposition: retained by `transport/tests.rs; engine/tests/sync_send.rs; engine/tests/rejected_before_write.rs; engine/tests/bootstrap_ownership.rs`. Generic send owner retains one write, exact claim/client evidence, busy inputs, refusal/uncertainty, settings and no PTY fallback. TUI-attachment delivery-state gates are retired.

- `codex_rpc_send_startup_requires_reservation_and_refuses_every_claimed_replay`
- `codex_rpc_send_startup_is_one_queued_turn_with_exact_id_and_overrides`

### `send/tests.rs`

Disposition: retained by `transport/tests.rs; engine/tests/sync_send.rs; engine/tests/rejected_before_write.rs; engine/tests/bootstrap_ownership.rs`. Generic send owner retains one write, exact claim/client evidence, busy inputs, refusal/uncertainty, settings and no PTY fallback. TUI-attachment delivery-state gates are retired.

- `codex_rpc_send_idle_user_starts_exactly_once`
- `lark_recording_contract_rpc_capture_ticket_denies_stale_identity_before_request`
- `lark_recording_contract_rpc_capture_valid_ticket_tracks_one_request`
- `lark_recording_contract_rpc_capture_uncertainty_never_pastes_or_replays`
- `lark_recording_contract_rpc_capture_cancel_after_steer_refusal_blocks_start`
- `lark_recording_contract_rpc_capture_hold_does_not_fall_back_to_paste`
- `codex_rpc_send_active_user_steers_exactly_once`
- `codex_rpc_send_active_agent_queues_one_new_turn`
- `codex_rpc_send_uncertain_user_start_never_replays`
- `codex_rpc_send_uncertain_start_never_replays`
- `codex_rpc_send_overrides_keep_reviewer_sandbox_cwd_model_and_effort`
- `codex_rpc_send_evidence_requires_user_message_client_id_notification`
- `codex_rpc_send_no_paste_after_rpc_handoff_or_uncertainty`
- `codex_rpc_send_prewrite_unavailability_uses_pty_exactly_once`
- `codex_rpc_send_unbound_sidecar_holds_without_paste`
- `codex_rpc_send_refused_ended_turn_starts_same_message_once`
- `codex_rpc_send_uncertain_or_unrelated_steer_error_never_replays`
- `codex_rpc_send_two_busy_messages_are_two_independently_confirmed_steers`

### `startup_retry/tests.rs`

Disposition: retired transport mechanism. Sidecar transport retry-to-PTY fallback is removed. Generic runtime exposes a failure and explicit exact-thread retry; it never selects a terminal fallback.

- `codex_rpc_startup_retry_transient_then_success_uses_sidecar`
- `codex_rpc_startup_retry_deterministic_refusal_is_attempted_once`
- `codex_rpc_startup_retry_exhaustion_returns_one_fallback`
- `codex_rpc_startup_retry_only_transport_failures_are_transient`
- `codex_rpc_startup_retry_superseded_launch_never_starts_another_server`
- `codex_rpc_startup_retry_diagnostics_preserve_transport_classification`

### `status.rs`

Disposition: retained by `subscriber/turn_lifecycle_tests.rs; plugins/provider-codex/tests/runtime.test.ts`. Own-turn waiting/idle/unknown and Compacting transitions remain generic; PTY classifier cannot change Chat-backed state.

- `codex_rpc_control_thread_status_preserves_waiting_idle_and_unknown`

### `thread_start/tests.rs`

Disposition: retained by `engine/tests.rs; engine/launch/tests.rs; engine/tests/bootstrap_ownership.rs; plugins/provider-codex/tests/runtime.test.ts`. Thread load/identity, resume, bootstrap and explicit model/effort settings remain generic; remote-TUI materialization is retired.

- `codex_rpc_instant_start_fresh_reply_identity_needs_no_history_or_mcp_events`
- `codex_rpc_instant_start_accepts_optional_analytics_source_only_on_own_reply`
- `codex_rpc_instant_start_remote_resume_waits_for_materialization_reply`
- `codex_rpc_instant_start_carries_resolved_model_effort_and_reviewer_explicitly`
- `native_thread_start_host_owned_config_matches_runtime_fixture`

### `transcript.rs`

Disposition: retained by `model/tests.rs; engine/backfill/tests.rs; plugins/provider-codex/tests/runtime.test.ts`. Generic cache/backfill retains stable row ids, order, exact own-thread and bounded startup paging.

- `cron_completion_requires_final_assistant_record_from_history`
- `codex_rpc_control_live_and_backfill_share_stable_item_ids`
- `codex_rpc_control_cached_snapshot_reuses_history_and_live_item_ids_in_order`

### `transport_config.rs`

Disposition: retained by `rpc/tests.rs`. Bounded stdio multi-MiB frames remain generic; WebSocket frame limits are retired.

- `codex_rpc_attach_websocket_accepts_measured_plugin_catalogue_frame`
- `codex_rpc_attach_websocket_accepts_single_frame_above_former_frame_limit`
- `codex_rpc_attach_websocket_retains_bounded_frame_memory`

### `unix_lease.rs`

Disposition: retained by `lease.rs`. The generic runtime retains the owner-lease group shutdown oracle on Unix.

- `closing_owner_lease_kills_the_sidecar_group`

## Consumer oracle receipts

- Removed `orchestrator/launch/preparation.rs` completion/channel tests: this asynchronous sidecar-only preparation no longer exists. PTY command is already prepared before the lifecycle owner writes it; existing launch-generation nonce/ownership tests remain.
- Removed five General Agent sidecar selection/token/defer tests: retired mechanism. Existing `general_agent/launch_files/tests.rs::restored_main_plain_launch_registers_exact_resume_authority_before_binding` remains; generic bootstrap overlap tests cover fresh/resumed sibling isolation and role/task claims.
- Removed `messenger/tests.rs::codex_rpc_send_messenger_preserves_routed_frame_and_queues_while_active`: generic `engine/tests/sync_send.rs::routed_runtime_envelope_reports_written_then_delivered_without_a_second_rpc` and `consecutive_distinct_runtime_envelopes_keep_independent_write_once_receipts` retain frame integrity/write-once receipts.
- Removed sidecar status observer test: generic `subscriber/turn_lifecycle_tests.rs` retains authoritative own-turn state. Existing provider-declared PTY event reconciliation test remains.
- Removed PTY runtime-mode selector test: remote-TUI settings control is retired. `StatusLineModelPicker.test.tsx` and `statusLinePickerModel.test.ts` retain capability-gated mode display, staged apply and bound values through the generic descriptor.
- Native/JS modes parity moved from the old structured-control test to `runtime_modes.rs` with a neutral fixture. No host function, filesystem, secret, network, authority or timeout permission was added.
- `flow_trace/source/tests.rs` retains sink delivery/link/count assertions; it no longer invokes the retired send or normalizer. Generic subscriber now links validated own-claim evidence and own-thread assistant items.
- `codexStructuredTools.test.tsx` retains the byte-identical mixed-turn fixture at its new local path.
- `mesh_rpc_delivery_tests.rs` retains typed zero-write refusal, panic uncertainty and local/mesh single invocation assertions without the obsolete route helper.
- `evals/restore-lifecycle/codex_oracle.py` requires current loaded runtime state and backing session for runtime rows; explicit PTY rows retain session/nonce/identity/user-count checks. Live owner/provider receipts are still required.

## Canonical red-green request

Before positive verification, run each rewritten/new isolated oracle against its deliberate sabotage, then restore source and run once:

- `codeterm-plugin` filter `parity_runtime_modes_native_js_describe`: sabotage JsProvider.agent_runtime() to return a capability with empty runtime_modes; expected failure is native/JS descriptor inequality.
- `codeterm-service-core` filter `runtime_control_confirmation_updates_recorded_model_and_next_turn_settings`: sabotage generic controls mode lookup back to an empty catalogue; expected failure is UnsupportedRuntimeMode for the declared fixture mode.
- `codeterm-service-core` filter `rpc_send_ingress_protocol_answer_reaches_sink_with_exact_client_turn_link`: omit trace_rpc_turn; expected failure is missing link/assistant chain.
- `python -m unittest discover -s evals/restore-lifecycle -p test_codex_oracle.py`: bypass loaded-runtime/backing-session requirements; expected failure is test_runtime_requires_loaded_own_thread_and_explicit_backing.

No sabotage or execution occurred in this lane under the owner verification freeze.

## Rollback boundary

Revert the complete batch, including the provider manifest/bundle and all consumers, as one unit. Do not restore only the RPC module or capability. Generic session/bootstrap/compaction/process ownership code is retained. No live process, provider hook or installed plugin was changed.

## Build cleanup dependency

Real-merged Sweep commit `a7e5d4820121f05103dacc370ed3204d4b8343fd` before removing the obsolete dev token-file reaper call and its two scripts/test files. Windows still uses its authenticated server/native shutdown path. Generic runtime process cleanup is unchanged. The deleted reaper tests prove only token-file/group ownership for the retired WebSocket mechanism; no current runtime creates those tokens. Retain Sweep's `scripts/tests/dev-app-process.test.mjs` canonical gate.

## Source handoff limits

The owner forbade all lane verification, including Node/TS tests, Rust checks and guards. The authorized metadata bump completed; `cargo hakari generate` completed with no generated changes, with Cargo metadata removing the direct service-core dependency from the lock. The one authorized `node scripts/build-plugin.mjs provider-codex` attempt exited 1 before output because esbuild is absent. The owner explicitly retains canonical plugin bundle regeneration and `build.sh fmt`; no install or formatting was attempted in this lane. Thus the committed plugin bundle remains stale until that canonical step, and generated-consumer acceptance is UNVERIFIED. No test, compiler-warning, guard or live-platform PASS is asserted.

Hook-authority negative oracle: `codeterm-api` selector `hook_routes::hook_authority` must reject telemetry ownership when either runtime capability or Chat backing is absent. For isolated red-green, sabotage authority to ignore the backing argument; the existing expanded fixture must reject the `(true, false)` case.

## Runtime transcript snapshot follow-up

The owner reported HTTP 500 `codechat_snapshot_failed` with `provider has no JSONL and no structured chat` for a runtime Chat tab on DEV `3b4b6982`, and the same missing source during session export. Source inspection at `77e7b5a3af649c06a3c59b0748b38b2258501b61` confirmed that tab snapshots use generic runtime history only for a live process; stored/waking fallback and tab-less session reads can miss the runtime entirely. That is a source finding, not a replay of the live incident in this lane.

`tab_chat/runtime_snapshot.rs` supplies the exact bound cache before tab source selection, including after process loss. Tab-less session reads select only an exact provider/cwd/session match and revalidate the current tab's reported nonce, backing identity, runtime epoch and binding around the cache read. Ambiguous owners are refused. No runtime is started to read a cached or closed-session transcript. Without a cached owner, the provider's existing bounded stored-runtime projection is used, preserving its start offset and own-session validation. PTY/ordinary Chat readers keep their existing sources. The API/export consumers already call these core readers; no Archive-owned surface was edited.

Four new isolated fixtures live in `tab_chat/runtime_snapshot/tests.rs`:

- `runtime_snapshot_reads_own_reply_without_jsonl_even_after_process_loss`: one exact assistant row through both tab and session snapshots; `since=1` returns zero rows while total remains one; the runtime process is absent and source selection is Stored.
- `runtime_snapshot_rejects_foreign_scope_nonce_and_backing_without_searching`: foreign provider/cwd/session, changed nonce/session and foreign backing cannot expose the cached reply. Session discovery panics if called.
- `runtime_session_snapshot_ignores_foreign_siblings_and_refuses_ambiguous_owners`: a sibling's different session never supplies the requested reply; two current owners of the exact scope are refused.
- `runtime_session_snapshot_reads_exact_closed_session_with_provider_projection`: one closed-session row comes through the runtime projection; foreign stored metadata is rejected and no runtime is created.

Canonical isolation-first red-green: disable the two new `runtime_snapshot` entry calls in `tab_chat/mod.rs`; the own-reply/no-store and closed-session projection tests must fail for missing/incorrect rows. Restore both calls and run the four-test selector once. Also sabotage `owns` to omit the reported-nonce/backing checks and remove the duplicate-owner refusal individually; the corresponding negative fixtures must fail. None of these demonstrations or tests ran in the lane under the owner's verification freeze. The HTTP J1 own-session reply and actual Archive export receipts remain UNVERIFIED until canonical runs them on the integrated SHA. Roll back this follow-up as one core-reader change; do not weaken nonce or session checks to recover data.

## Canonical compile bounce at 4dc6fbda19

Canonical supplied compiler errors at `4dc6fbda198f6465b0a02d3f9367f29a01d091cd`: three references to removed `structured` launch state, unused deferred nonce export and command-context binding, and a missing public model-switch method in runtime tests. The lane real-merged that exact integration SHA, retaining the runtime snapshot follow-up. The public model-control method had also served the API despite living inside the deleted RPC module; it now lives in generic runtime controls, selects by capability/backing, and preserves rejection of unsupported tier/mode controls before PTY fallback. Runtime controls still never select a terminal fallback.

All PTY launch readiness now waits on the authenticated observer; no obsolete sidecar branch can synthesize Ready. The unused deferred sidecar nonce helper/export is gone. Its existing reservation test keeps the exact pinned-send/registration assertions and proves invalidation through the real nonce rotation API. The source-order readiness assertion now locates the observer declaration and still requires its receive before ProviderReady emission.

The owner authorized exactly the multi-package lane check with `--tests --wait-build` and the separate CLI lane check with the same flags, once each, and no tests. Results are recorded in the post-check report tied to its own SHA; this paragraph does not assert an execution result. Existing live J1, export, red-green and platform observations remain pending until their owners record receipts.
