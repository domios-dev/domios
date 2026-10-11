import { describe, expect, it } from "vitest";
import { createOrchestrationTransport, type OrchestrationClient } from "./orchestration";

function fixture(response: unknown = {}) {
  const calls: { method: string; path: string; body: unknown; opts: unknown }[] = [];
  const client: OrchestrationClient = {
    async json<T>(method: string, path: string, body?: unknown, opts?: unknown): Promise<T> {
      calls.push({ method, path, body, opts });
      return response as T;
    },
  };
  return { calls, transport: createOrchestrationTransport(client) };
}

describe("orchestration transport", () => {
  it("unwraps plan list envelopes and preserves project scope and cancellation", async () => {
    const plans = [{ id: "fixture" }];
    const { calls, transport } = fixture({ plans });
    const signal = new AbortController().signal;
    expect(await transport.list_plan_index({ signal })).toEqual(plans);
    expect(await transport.list_plans_full({ project: "/a b" }, { signal })).toEqual(plans);
    expect(calls.map(c => c.path)).toEqual(["/api/plans", "/api/plans?full=1&project=%2Fa+b"]);
    expect(calls[1].opts).toEqual({ signal });
    await transport.get_plan_full({ planId: "plan/x" });
    expect(calls[2].path).toBe("/api/plans/plan%2Fx");
  });

  it("keeps Blueprint scope explicit and carries full node identity and command route", async () => {
    const { calls, transport } = fixture();
    await transport.automation_graph_snapshot({ workspaceId: "workspace", teamId: null });
    expect(calls[0].path).toBe("/api/automation/graph?workspace_id=workspace&team_id=");
    expect(() => transport.automation_graph_snapshot({ workspaceId: "workspace", teamId: undefined as never })).toThrow();
    const identity = { kind: "blueprint" as const, id: "b", workspace_id: "workspace", team_id: "team", owner_tab_id: "owner" };
    await transport.automation_node_files({ identity, bucket: "done", handler: "script" });
    expect(calls[1]).toMatchObject({ method: "POST", path: "/api/automation/node-files", body: { identity, bucket: "done", handler: "script" } });
    await transport.automation_graph_stop_blueprint({ workspaceId: "w", teamId: "t", id: "b/x", callerTabId: "owner", sessionBindingNonce: "nonce" });
    expect(calls[2]).toMatchObject({ path: "/api/automation/blueprints/b%2Fx/stop", body: { workspace_id: "w", team_id: "t", caller_tab_id: "owner", session_binding_nonce: "nonce" } });
  });

  it("uses selected tab and preserves returned bridge state", async () => {
    const state = { bridge_active: true, topics: "available", bound: true, link: "fixture-link" };
    const { calls, transport } = fixture(state);
    expect(await transport.bridge_thread_state({ tabId: "target/x" })).toEqual(state);
    await transport.bridge_thread_open({ tabId: "target/x", callerTabId: "caller", sessionBindingNonce: "nonce" });
    await transport.bridge_thread_close({ tabId: "target/x" });
    expect(calls.map(c => c.method)).toEqual(["GET", "POST", "DELETE"]);
    expect(calls.every(c => c.path === "/api/tabs/target%2Fx/bridge/thread")).toBe(true);
    expect(calls[1].body).toEqual({ selected_tab_id: "target/x", caller_tab_id: "caller", session_binding_nonce: "nonce" });
  });

  it("passes HTTP refusal through without retry or fallback", async () => {
    const error = Object.assign(new Error("Forbidden"), { status: 403 });
    let count = 0;
    const transport = createOrchestrationTransport({ async json() { count++; throw error; } });
    await expect(transport.bridge_thread_open({ tabId: "target" })).rejects.toBe(error);
    expect(count).toBe(1);
  });

  it("maps every cron, team, Blueprint and orchestration command to its existing route", async () => {
    const { calls, transport } = fixture();
    await transport.activity_list();
    await transport.cron_list({ workspaceId: "w/x" });
    await transport.cron_get({ id: "c" });
    await transport.cron_history({ id: "c" });
    await transport.cron_delete({ id: "c" });
    await transport.cron_trigger({ id: "c" });
    await transport.cron_cancel({ id: "c" });
    await transport.cron_toggle({ id: "c", enabled: false });
    await transport.cron_update({ id: "c", params: { enabled: true } });
    await transport.mux_rename_team({ teamId: "t", name: "Name", callerTabId: "caller", sessionBindingNonce: "nonce" });
    await transport.mux_close_team({ teamId: "t", forceBusy: true });
    await transport.mux_add_team_member({ teamId: "t", provider: "provider", role: "worker", brief: "opaque" });
    await transport.mux_make_orchestrator({ tabId: "tab" });
    await transport.mux_make_planner({ tabId: "tab", planId: "plan" });
    await transport.mux_create_orchestration_pair({ params: { orchestrator_agent: "provider" } });
    await transport.automation_graph_workspace_snapshot({ workspaceId: "w" });
    await transport.automation_graph_delete_blueprint({ workspaceId: "w", teamId: null, id: "b" });
    await transport.blueprint_export_file({ workspaceId: "w", teamId: null, id: "b" });
    await transport.blueprint_import_file({ callerTabId: "caller", recipe: {}, assignments: [] });
    expect(calls.map(c => [c.method, c.path])).toEqual([
      ["GET", "/api/activity"], ["GET", "/api/crons?workspace_id=w%2Fx"], ["GET", "/api/crons/c"], ["GET", "/api/crons/c/history"],
      ["DELETE", "/api/crons/c"], ["POST", "/api/crons/c/trigger"], ["POST", "/api/crons/c/cancel"], ["POST", "/api/crons/c/disable"], ["PATCH", "/api/crons/c"],
      ["POST", "/api/teams/t/rename"], ["POST", "/api/teams/t/close"], ["POST", "/api/teams/t/members"], ["POST", "/api/tabs/tab/make-orchestrator"],
      ["POST", "/api/tabs/tab/make-planner"], ["POST", "/api/orchestration/new"], ["GET", "/api/automation/graph/workspace?workspace_id=w"],
      ["DELETE", "/api/automation/blueprints/b"], ["POST", "/api/automation/blueprints/export"], ["POST", "/api/automation/blueprints/import"],
    ]);
    expect(calls[10].body).toMatchObject({ force: true, confirm: "t", force_busy: true });
    expect(calls[11].body).toMatchObject({ task: "opaque" });
  });
});

it("ends old requests and ignores old SSE after a client replacement", async () => {
  const { bindOrchestrationTransport, orchestrationTransport, listenOrchestration } = await import("./orchestrationRuntime");
  const { orchestrationTestClient } = await import("./orchestration.testdata");
  let oldEvent!: (name: string, data: unknown) => void;
  let oldSignal: AbortSignal | undefined;
  let closed = 0;
  bindOrchestrationTransport(orchestrationTestClient(async <T,>(_method: string, _path: string, _body?: unknown, opts?: { signal?: AbortSignal }) => {
    oldSignal = opts?.signal;
    return [] as T;
  }, (_stream, onEvent) => {
    oldEvent = onEvent;
    return () => { closed++; };
  }));
  await orchestrationTransport().cron_list();
  const seen: unknown[] = [];
  const stop = await listenOrchestration("cron-run-finished", ({ payload }) => seen.push(payload));
  oldEvent("other", "ignored");
  oldEvent("cron-run-finished", "first");
  bindOrchestrationTransport(orchestrationTestClient(async <T,>() => [] as T));
  oldEvent("cron-run-finished", "late");
  stop();
  expect(oldSignal?.aborted).toBe(true);
  expect(closed).toBe(1);
  expect(seen).toEqual(["first"]);
});

it("forwards the exact claim on all guarded team commands and refuses missing caller nonce", async () => {
  const { calls, transport } = fixture();
  const claim = { callerTabId: "selected", sessionBindingNonce: "selected-current-nonce" };
  await transport.mux_rename_team({ teamId: "team", name: "Name", ...claim });
  await transport.mux_close_team({ teamId: "team", ...claim });
  await transport.mux_add_team_member({ teamId: "team", provider: "provider", role: "worker", ...claim });
  await transport.mux_make_orchestrator({ tabId: "selected", ...claim });
  await transport.mux_make_planner({ tabId: "selected", planId: "plan", ...claim });
  await transport.mux_create_orchestration_pair({ params: { workspace_id: "workspace" }, ...claim });
  expect(calls).toHaveLength(6);
  for (const call of calls) expect(call.body).toMatchObject({ caller_tab_id: "selected", session_binding_nonce: "selected-current-nonce" });
  const missing = { callerTabId: "missing" };
  expect(() => transport.mux_rename_team({ teamId: "team", name: "Name", ...missing })).toThrow();
  expect(() => transport.mux_close_team({ teamId: "team", ...missing })).toThrow();
  expect(() => transport.mux_add_team_member({ teamId: "team", provider: "p", role: "worker", ...missing })).toThrow();
  expect(() => transport.mux_make_orchestrator({ tabId: "missing", ...missing })).toThrow();
  expect(() => transport.mux_make_planner({ tabId: "missing", planId: "p", ...missing })).toThrow();
  expect(() => transport.mux_create_orchestration_pair({ params: {}, ...missing })).toThrow();
  expect(calls).toHaveLength(6);
});
