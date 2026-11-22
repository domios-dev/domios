# Provider switch

Use `domios agent switch-provider TARGET --tab TAB [--profile ID] [--model MODEL] [--reasoning-effort EFFORT]` to continue on another installed provider. Targets declaring launch profiles require `--profile ID` and validate that profile's installation before retiring the source tab. The target receives a bounded handoff containing recent user/assistant messages and host-owned task context, and produces its working summary there. The source provider need not summarize anything.

The tab id, title, description, role, Team, parent, lane/cwd, pad and task attempt remain. The old provider process is retired, and the result names its transcript archive. Full history is not transferred. A busy tab refuses; wait for idle or explicitly pass `--force` to stop its current turn first. Unsupported lifecycle capabilities and unavailable installation or authentication refuse honestly.

Self-switch is supported from the tab’s current caller identity, including the protected General (`--tab main`). The daemon owns completion after retiring that process. The General keeps its aliases, role and bridge/Telegram routing; a successful switch also saves its provider/model/effort defaults for restart, restore and fresh General launches. Ordinary tabs never change those defaults.

REST: `POST /api/tabs/{id}/agent/switch-provider` with `{provider, launch_profile?, model?, reasoning_effort?, force?}`. The daemon returns HTTP 202 with `operation_id`, `tab_id`, `provider` and `phase: pending` before retiring any caller. Use `domios agent switch-provider-status OPERATION --tab TAB` or `GET /api/tabs/{id}/agent/switch-provider/{operation}` to read `pending`, `running`, `completed` or `failed` plus `error`. A completed `outcome` includes `session_id`, `previous_provider`, `previous_session_id`, `previous_history_path`, `binding_restored: true` and `handoff_delivered: true`. UI clients can pass `?wait=true` to wait for the completion signal (bounded at 180 seconds); a still-pending result is not a failure or permission to retry. A delivery uncertainty never replays the handoff automatically.

The optional `stage` records host-owned progress: `preflight`, `source_retirement`, `target_launch`, `target_binding`, `target_ready`, then `handoff`. A status-wait deadline leaves the operation running. A launch observer deadline records uncertainty and preserves a surviving target; late exact binding may update that same operation to target ready, but an unsent handoff remains failed and is never replayed. Inspect the exact operation and its retained source transcript before choosing recovery.

Read a returned handoff archive with `GET /api/tabs/{id}/agent/provider-history?file=PATH`. It accepts only an inbox archive within that tab's own pad and returns transcript message rows. In the app, earlier turns stay visible in the tab's chat.

<!-- revision: 5 -->
