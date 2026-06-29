**HOW the CLI drives the canvas.** A surface over the plan store (NOT mem-graph — plans stay JSON) gives agents the verbs to see and pick up work, plus a focus verb that flies the open board to a target. The focus path is the interesting one: `codeterm canvas focus --task|--pane|--plan` posts to a route that emits over the existing `emit_everywhere` fan-out, so *every* open board reacts:

```rust
// src-tauri/src/api/task_routes.rs
pub async fn canvas_focus(State(state): State<AppState>, Json(body): Json<CanvasFocusBody>) -> TaskRouteResult {
    let payload = json!({ "task_id": body.task_id, "pane_id": body.pane_id, "plan_id": body.plan_id });
    crate::api::events::emit_everywhere(&state.svc.app_handle, "canvas-focus", payload.clone());
    Ok(Json(json!({ "ok": true, "event": "canvas-focus", "payload": payload })))
}
```

The frontend resolves that payload to concrete node ids — and a `pane_id` resolves to *that pane's assigned tasks*, which is how the per-pane **Plan** button focuses the camera on the area an agent works on:

```ts
// canvasFocusPayload.ts
export function resolveCanvasFocusNodeIds(payload, plans): string[] {
  const ids = [...(payload.node_ids ?? []), ...(payload.plan_ids ?? [])];
  if (payload.task_id) ids.push(payload.task_id);
  if (payload.plan_id) ids.push(payload.plan_id);
  if (payload.pane_id)
    for (const plan of plans) for (const t of plan.tasks)
      if (t.assigned_pane_id === payload.pane_id) ids.push(t.id);
  return ids;
}
```

The board then `animateToView`s the union bbox of those nodes and pulses them. `flyToTarget.ts::resolveFlyToNodeId` maps domain ids (a spec section, a `follows`/`supersedes` lineage chip) onto the actual canvas node to land on. The read verbs — `task list/show/take/drop/add`, `plan gaps` — round out "what's in flight / what can I pick up / what's uncovered".

> Note: the BE→FE contract had a real bug (route emitted `{task_id}` while a listener read `{node_ids,plan_ids}`); the fix is exactly the shared `resolveCanvasFocusNodeIds` above, so emit-shape == read-shape end to end (`task_dbb511f6`).

## Key decisions
- **Reuse `emit_everywhere` over a focus-specific channel** — focus rides the same event fan-out as `plan-changed`; any board, any window, reacts with no new transport.
- **`pane_id` → assigned tasks resolution in one shared function** — the same resolver serves the CLI, the Plan button, and tests, so the contract can't drift again.
- **Plan store, not mem-graph** — plans stay authoritative JSON; mem is only ever an index (see *Project Plan Memory Sidecar*).
