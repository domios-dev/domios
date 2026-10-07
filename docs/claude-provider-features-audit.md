# Claude provider feature consumer audit

Source-only lane: no Rust compilation, project tests, live checks or generated artifacts were run here.

## Lifecycle consumers

`service/tab_chat/structured_sink.rs` and `commands/tab.rs` feed own-session observations into `service/background_work.rs`; runtime observations retain session provenance across hydration. The per-tab roster is discarded on close and reset on session or launch identity changes. Forks exclude copied source transcript work. `background_work_guard` defaults false.

`service/agent_restart.rs` and `service/agent_restart/model_switch.rs` refuse destructive restarts with typed running items. `service/provider_update/restarts.rs` defers and is awakened by `provider_update::background_work_changed`, with a drain-race recheck. Explicit model `--force` produces an owner-visible warning.

## Fork contract consumers

`cli/src/args/agent.rs`, `cli/src/cmd/agent.rs`: `codeterm agent fork --tab ID`, and model switch `--force`. REST `api/agent_fork_routes.rs`, Tauri `commands/agent.rs`, web `src/lib/bridge.ts`, shared tab menu `tabMenuModel.ts` / `tabMenuActions.ts`, SDK / manifest fork declaration and provider descriptor all use the capability. Mesh explicitly refuses this local lifecycle action. The CLI primer is updated in `src-tauri/prompts/codeterm_cli.md`.

The new tab resumes the source with the declared fork arguments and never pre-binds the source or an invented session id. Binding requires a distinct session plus launch evidence; hook claims require the exact nonce. Launch input remains on the existing bootstrap path. Source tab state is untouched.

## Canonical gates requested

Regenerate the Claude bundle with `node scripts/build-plugin.mjs provider-claude`, then CLI contracts with `.\build.ps1 cli --wait-build`, committing generated artifacts in canonical. Run `.\build.ps1 check-tests --wait-build`; `bash scripts/plugin-tests.sh provider-claude`; `.\build.ps1 test-one plugin parity_claude_ --wait-build`; `.\build.ps1 test-one types restart_guard_ --wait-build` if the types target is supported, otherwise select the background roster tests through the canonical runner; `.\build.ps1 test-one app restart_guard_ --wait-build`; `.\build.ps1 test-one app model_switch --wait-build`; `.\build.ps1 test-one app agent_fork_ --wait-build`; and the focused `tabMenuModel.test.ts` TypeScript selector.

Expected: nonzero selected cases and zero failures; active shells/agents named in typed refusals; completion drains the roster and permits one deferred restart; force emits a warning; fork source stays bound while the new tab binds a different evidenced id. Verify those observable states live only in owner DEV via `evals/_lib/lib.sh` and `dc`. Audit the regenerated `generated/cli` diff and ensure regeneration is clean afterward.
