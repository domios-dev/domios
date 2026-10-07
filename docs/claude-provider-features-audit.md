# Claude provider feature consumer audit

Source-only lane: no Rust compilation, project tests, live checks or generated artifacts were run here.

## Lifecycle consumers

`service/tab_chat/structured_sink.rs` and `commands/tab.rs` feed own-session observations into `service/background_work.rs`; runtime observations retain session provenance across hydration. The per-tab roster is discarded on close and reset on session or launch identity changes. Forks exclude copied source transcript work. `background_work_guard` defaults false.

`service/agent_restart.rs` and `service/agent_restart/model_switch.rs` refuse destructive restarts with typed running items. `service/provider_update/restarts.rs` defers and is awakened by `provider_update::background_work_changed`, with a drain-race recheck. Explicit model `--force` produces an owner-visible warning.

## Fork contract consumers

`cli/src/args/agent.rs`, `cli/src/cmd/agent.rs`: `codeterm agent fork --tab ID`, and model switch `--force`. REST `api/agent_fork_routes.rs`, Tauri `commands/agent.rs`, web `src/lib/bridge.ts`, shared tab menu `tabMenuModel.ts` / `tabMenuActions.ts`, SDK / manifest fork declaration and provider descriptor all use the capability. Mesh explicitly refuses this local lifecycle action. The CLI primer is updated in `src-tauri/prompts/codeterm_cli.md`.

The new tab resumes the source with the declared fork arguments and never pre-binds the source or an invented session id. Binding requires a distinct session plus launch evidence; hook claims require the exact nonce. Launch input remains on the existing bootstrap path. Source tab state is untouched.

## Canonical gates requested

Regenerate the Claude bundle with `node scripts/build-plugin.mjs provider-claude`, then CLI contracts with `.\build.ps1 cli --wait-build`, committing generated artifacts in canonical. Run `.\build.ps1 check-tests`; `bash scripts/plugin-tests.sh provider-claude`; `.\build.ps1 test-one plugin parity_claude_ --wait-build`; `.\build.ps1 test-one types restart_guard_ --wait-build`; `.\build.ps1 test-one app restart_guard_ --wait-build`; `.\build.ps1 test-one app model_switch --wait-build`; `.\build.ps1 test-one app agent_fork_ --wait-build`; and the focused `tabMenuModel.test.ts` TypeScript selector.

Expected: nonzero selected cases and zero failures; active shells/agents named in typed refusals; completion drains the roster and permits one deferred restart; force emits a warning; fork source stays bound while the new tab binds a different evidenced id. Verify those observable states live only in owner DEV via `evals/_lib/lib.sh` and `dc`. Audit the regenerated `generated/cli` diff and ensure regeneration is clean afterward.

## Model-free initialize consumer audit

The manifest declares `spawn.capabilityProbe`, native and JS expose the same typed capability, and core selects the protocol by that capability. Parsing stays in core; the JS sandbox gains no host function. The per-provider `capability_probe_enabled` setting defaults false.

`service/capability_probe/process.rs` directly executes the resolved absolute binary, strips CodeTerm environment variables, disables hooks and persistence, and sends exactly one initialize control line. It reuses `provider_update::ProcessTree` for Windows suspended attach into a kill-on-close job and Unix process groups. Control response and job completion use bounded event waits. There is no user message, shell or usage query. Output is bounded at 1 MiB; cache entries at 64 projects. One event-driven executor contains at most one initialize child at a time, with a bounded queue and a deadline that includes queue time; expired jobs never spawn. Cache keys contain canonical project identity and binary fingerprint; monotonic entry generations reject late results after refresh/invalidation.

`agent/model_binary_watch.rs` invalidates on declared catalogue input events and settings changes; `provider_signals::refresh_auth` invalidates on an observed auth change. Lifecycle/auth events request a cached refresh off-runtime. Explicit refresh is REST `POST /api/tabs/{id}/agent/capabilities/refresh`, Tauri `refresh_provider_capabilities`, and the composer badge Refresh control. Render, tab switch and send do not execute a probe.

`src/components/Terminal/ChatComposer.tsx` consumes cached command names/descriptions/argument hints through `src/lib/providerCapabilities.ts`; `ProviderCapabilityBadge.tsx` displays subscription, API provider, permission mode, fast mode and typed unavailable state. The model merge point is `service/capability_probe.rs::merge_models`, using a separate `model_catalog::put_capability_overlay` project overlay that survives ordinary live catalogue refreshes and preserving existing declared effort metadata. New aliases with unknown effort capabilities expose an empty effort set instead of inventing it. Account email and tokens are not retained.

Source review also tightened lifecycle safety: truncated initial background history refuses a guarded restart until explicit force or a fresh session; General Agent teardown honors the roster before clearing its binding; fork launch history does not record the source as a target binding.

Canonical Part C gates: `.\build.ps1 test-one app provider_update_process_ --wait-build` rechecks the reused containment helper; `.\build.ps1 test-one app claude_init_probe_ --wait-build` selects 7 authored cases (pure decode/cache/merge plus direct Node fake binary success/deadline and Windows child-exit receipts); `.\build.ps1 test-one plugin parity_claude_initialize_capability_declaration --wait-build` selects 1 case; `.\build.ps1 test-one plugin claude_init_probe_ --wait-build` selects the catalogue overlay regression; `.\build.ps1 test-one pty restart_guard_ --wait-build` selects the tab-close isolation regression; `.\build.ps1 test-one cli agent_fork_and_model_force_parse --wait-build` selects 1 case. Run `node scripts/run-ts-unit-tests.mjs src/lib/providerCapabilities.test.ts src/components/Terminal/tabMenuModel.test.ts` (3 new cached consumer cases plus existing menu cases) and `node node_modules/typescript/bin/tsc --noEmit`. Repeat the canonical check-tests and all Claude plugin/parity selectors after regenerated bundles.

Owner DEV oracle: explicitly enable `capability_probe_enabled`, call the authenticated refresh route through the dev wrapper, and observe a ready typed response and the command hints / live aliases / subscription badge. Before and after, confirm no new `~/.claude/projects` directory, no probe SessionStart hook, no model turn, and no bound real tab from the probe. Disable the switch until this oracle passes. The source-only lane did not execute these checks.

The guarded roster has a separate `service/background_work_watch.rs` observer, started on binding/lifecycle state events when structured state, the guard, or the capability probe is enabled. It watches only the claimed transcript and its nearest existing Claude data ancestor, blocks on filesystem event channels, bounds reads to 8 MiB, holds partial records, and refuses unobserved/truncated history. It does not depend on opening Chat UI and introduces no polling loop. Closing the tab cancels its watcher; generation identity rejects old-session callbacks. It also feeds typed transcript health and compaction to their existing consumers. Canonical `app restart_guard_` includes two event-reader boundary cases; `types restart_guard_` includes unobserved/incomplete-history refusals.

Binding audit: the legacy post-launch newest-session fallback and manual `mux_set_claude_session_id` cannot bind a fork. Fork claims flow through the nonce-verified hook / launch-detection claim path only; an explicit manual attempt returns refusal.
