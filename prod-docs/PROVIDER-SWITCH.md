# Provider switch

Use `codeterm agent switch-provider TARGET --tab TAB [--model MODEL] [--reasoning-effort EFFORT]` to continue on another installed provider. The target receives a bounded handoff containing recent user/assistant messages and host-owned task context, and produces its working summary there. The source provider need not summarize anything.

The tab id, title, description, role, Team, parent, lane/cwd, pad and task attempt remain. The old provider process is retired, and the result names its transcript archive. Full history is not transferred. A busy tab refuses; wait for idle or explicitly pass `--force` to stop its current turn first. Unsupported lifecycle capabilities and unavailable installation or authentication refuse honestly.

Self-switch is supported from the tab’s current caller identity, including the protected General (`--tab main`). The daemon owns completion after retiring that process. The General keeps its aliases, role and bridge/Telegram routing; a successful switch also saves its provider/model/effort defaults for restart, restore and fresh General launches. Ordinary tabs never change those defaults.

REST: `POST /api/tabs/{id}/agent/switch-provider` with `{provider, model?, reasoning_effort?, force?}`. The daemon returns HTTP 202 with `operation_id`, `tab_id`, `provider` and `phase: pending` before retiring any caller. Use `codeterm agent switch-provider-status OPERATION --tab TAB` or `GET /api/tabs/{id}/agent/switch-provider/{operation}` to read `pending`, `running`, `completed` or `failed` plus `error`. A completed `outcome` includes `session_id`, `previous_provider`, `previous_session_id`, `previous_history_path`, `binding_restored: true` and `handoff_delivered: true`. UI clients can pass `?wait=true` to wait for the completion signal (bounded at 180 seconds); a still-pending result is not a failure or permission to retry. A delivery uncertainty never replays the handoff automatically.

Read a returned handoff archive with `GET /api/tabs/{id}/agent/provider-history?file=PATH`. It accepts only an inbox archive within that tab's own pad and returns transcript message rows. The confirmation dialog exposes it as Previous provider after a successful switch.

<!-- revision: 1 -->
