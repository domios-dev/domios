# watcher-health

Retired bench. Its provider plugin was removed; its launcher also used fixed
sleeps and transcript polling. There is no executable smoke test in this folder.
Retirement does not establish a runtime PASS for any of its three acceptance cases.

## Retained acceptance cases

| Former oracle | Retained source coverage | Remaining observation |
|---|---|---|
| Spawn a watcher-capable backend under an orchestrator with a charter | `watcher_spawn_params_route_to_machine_watcher_config_and_spec` and `watcher_spawn_validation_rejects_non_watcher_capable_chatbackend` in `codeterm-service-core` | A current backend must declare the watcher capability and acknowledge a real isolated spawn. |
| Receive context and a verdict in the watcher transcript | `watcher_verdict_apply_path_clears_inflight_updates_state_and_yields_actions` in `codeterm-service-core` | Observe both exact transcript event kinds from that backend's owned session. |
| Deliver one nudge across two ticks with cooldown | `watcher_due_tab_fires_exactly_one_tick_per_interval` in `codeterm-service-core`; `guardrail_cooldown_suppresses_recent_nudge` and `guardrail_dedupes_duplicate_nudge_targets_within_tick` in `codeterm-worker-services::watcher::verdict` | Observe two acknowledged ticks and exactly one delivered nudge in an isolated orchestrator thread. |

These checks cover host decisions, not the missing backend or live delivery.
The live rows remain UNVERIFIED. Any replacement bench must use the dev-locked
`evals/_lib/lib.sh`, owned fixtures, capability discovery and event acknowledgements;
it must fail on missing prerequisites rather than reporting a successful skip.

## Current endpoint connections and historical evidence

Rotterdom is the official optional harness for OpenAI-compatible endpoints.
Its current manifest declares chat-backend and decision-model capabilities;
it does not declare watcher support. Discover installed capabilities through
`domios capabilities` before choosing a backend; an endpoint connection alone
does not establish watcher support. LM Studio remains a legitimate endpoint
brand in product help. Recorded LM Studio transcripts, measurements and past
failed launches in `evals/tab-state-jev/` remain historical evidence.
