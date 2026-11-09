import { muxMockFetch } from "../../lib/muxMockFetch.testdata";
import assert from "node:assert/strict";
import { Window } from "happy-dom";
import type { ApiConfig } from "../../lib/types";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";

async function main() {
  const win = new Window({ url: "http://gateway:7685" });
  Object.assign(globalThis, { window: win, localStorage: win.localStorage, sessionStorage: win.sessionStorage });
  const { setRelayTarget, clearRelayTarget, setTokenFor, invalidateWebRequestContext } = await import("../../lib/webApi");
  const plansApi = await import("./plansApi");
  const { fetchPlanSkills, fetchPlanSkill } = await import("./planView/planSkillsApi");
  const { fetchPlanBoardState, fetchTeamMissionControlState } = await import("./missionControl/missionControlState");
  const { fetchTeamSpend } = await import("./missionControl/teamSpend");
  const { createCanvasPlanRequest, spawnOrchestrator } = await import("./canvasApi");
  const API: ApiConfig = { enabled: true, host: "peer", port: 7685, token: "unused-peer-token", cors: true };
  setTokenFor("http://gateway:7685", "gateway-token");

  type Request = { method: string; path: string; body: unknown; target_peer_id?: string };
  const requests: Request[] = [];
  const remotePlan = { ...FIXTURE_PLAN, revision: "1", id: "pl_peer", project_path: "/home/peer/project" };
  const index = { plan_id: remotePlan.id, title: remotePlan.title };
  const skill = { slug: "check", title: "Check", body: "opaque", ledger: [] };
  const taskBoardPage = {
    plan_id: remotePlan.id, revision: "1",
    column: {
      status: "done",
      items: [{ id: "task-1", position: 4, created_at: 7, name: "Task", status: "done" }],
      next_cursor: null,
    },
  };
  const taskDetail = {
    plan_id: remotePlan.id, revision: "1",
    task_id: "task-exact",
    task: { id: "task-exact", name: "Peer detail" },
    context: { plan_id: remotePlan.id, title: remotePlan.title, status: "active", owner: remotePlan.owner, sections: [] },
  };

  globalThis.fetch = muxMockFetch(async (input, init) => {
    if (init?.signal?.aborted) throw new DOMException("Aborted", "AbortError");
    const url = new URL(String(input));
    assert.equal(new Headers(init?.headers).get("Authorization"), "Bearer gateway-token");
    if (url.pathname !== "/api/mesh/relay-to") return Response.json({ plans: [] });
    const request = JSON.parse(String(init?.body)) as Request;
    requests.push(request);
    let data: unknown = remotePlan;
    if (request.path.startsWith("/api/plans?")) data = { plans: request.path.includes("view=summary") ? [remotePlan] : [index] };
    if (request.path === "/api/plans") data = { plans: [index] };
    if (request.path === `/api/plans/${remotePlan.id}/tasks/task-exact`) data = taskDetail;
    if (request.path.startsWith(`/api/plans/${remotePlan.id}/tasks?`)) data = taskBoardPage;
    if (request.path.endsWith("/reconcile")) data = { drift: [{ id: "peer-drift" }] };
    if (request.path === "/api/plans/resolve-refs") data = { refs: [{ plan_id: remotePlan.id }] };
    if (request.path.startsWith("/api/specs/")) data = { spec: { sections: [{ id: "peer-section" }] } };
    if (request.path.endsWith("/skills")) data = { skills: [skill] };
    if (request.path.endsWith("/skills/check")) data = { skill };
    return Response.json({ ok: true, status: 200, data, error: null });
  });

  setRelayTarget("ubunta", "ubunta");
  assert.deepEqual((await plansApi.fetchPlans(API)).map((plan) => plan.id), [remotePlan.id]);
  assert.deepEqual((await plansApi.fetchPlanIndex(API)).map((plan) => plan.plan_id), [remotePlan.id]);
  assert.equal((await plansApi.fetchPlanProjection(API, remotePlan.id, "document")).id, remotePlan.id);
  const taskPage = await plansApi.fetchPlanTaskBoardPage(API, remotePlan.id, {
    status: "done",
    cursor: { created_at: 3, position: 2, task_id: "cursor-task" },
  });
  assert.deepEqual(await plansApi.fetchPlanTaskDetail(API, remotePlan.id, "task-exact"), taskDetail);
  assert.deepEqual(taskPage, taskBoardPage);
  assert(requests.some((request) => request.path === `/api/plans/${remotePlan.id}/tasks/task-exact`));
  assert(requests.some((request) => request.path === `/api/plans/${remotePlan.id}/tasks?page_size=25&status=done&after_created_at=3&after_position=2&after_task_id=cursor-task`));
  assert.deepEqual(await plansApi.fetchSpecSections(API, "spec/peer"), [{ id: "peer-section" }]);
  assert.deepEqual(await plansApi.fetchReconcile(API, remotePlan.id), [{ id: "peer-drift" }]);
  assert.deepEqual(await plansApi.resolvePlanRefs(API, [{ plan_id: remotePlan.id, target_kind: "plan" }]), [{ plan_id: remotePlan.id }]);
  assert.deepEqual(await plansApi.fetchUnownedPlanIndex(API), [index]);
  assert.deepEqual(await fetchPlanSkills(API, remotePlan.id), [skill]);
  assert.deepEqual(await fetchPlanSkill(API, remotePlan.id, skill.slug), skill);
  await fetchPlanBoardState(API, remotePlan.id);
  await fetchTeamMissionControlState(API, "team-peer");
  await fetchTeamSpend(API, "team-peer", { metric: "tokens" });
  await createCanvasPlanRequest(API, { title: "Peer plan" });
  await spawnOrchestrator(API, { providerId: "fixture-provider", workspaceId: "peer-workspace" });
  await plansApi.fetchPlans(API, undefined, { project: remotePlan.project_path });
  await plansApi.renamePlan(API, remotePlan.id, "Peer title");
  assert.equal(requests[requests.length - 1]?.method, "PATCH");
  assert.deepEqual(requests[requests.length - 1]?.body, { title: "Peer title" });
  assert(requests.some((request) => request.path === "/api/plans?view=summary&project=%2Fhome%2Fpeer%2Fproject"));
  assert(requests.some((request) => request.path === "/api/specs/spec%2Fpeer"));
  assert(requests.every((request) => request.target_peer_id === "ubunta"));
  assert(requests.some((request) => request.path === "/api/teams/team-peer/mission-control/state"));
  assert(requests.some((request) => request.path === "/api/teams/team-peer/mission-control/spend?metric=tokens"));

  const ctrl = new AbortController();
  ctrl.abort();
  await assert.rejects(plansApi.fetchPlans(API, ctrl.signal), { name: "AbortError" });
  await assert.rejects(plansApi.fetchPlanTaskBoardPage(API, remotePlan.id, { signal: ctrl.signal }), { name: "AbortError" });
  await assert.rejects(plansApi.fetchPlanTaskDetail(API, remotePlan.id, "task-exact", ctrl.signal), { name: "AbortError" });

  let finishOld!: (value: Response) => void;
  let dispatched!: () => void;
  const reached = new Promise<void>((resolve) => { dispatched = resolve; });
  globalThis.fetch = muxMockFetch(() => new Promise<Response>((resolve) => { finishOld = resolve; dispatched(); }));
  const oldRead = plansApi.fetchPlans(API);
  // The carrier dispatches after its ack, so the read is in flight only once the daemon has it.
  await reached;
  invalidateWebRequestContext();
  setRelayTarget("eight", "eight");
  finishOld(Response.json({ ok: true, status: 200, data: { plans: [remotePlan] } }));
  await assert.rejects(oldRead, { name: "AbortError" });

  const failedPaths: string[] = [];
  globalThis.fetch = muxMockFetch(async (input) => {
    failedPaths.push(new URL(String(input)).pathname);
    return Response.json({ ok: false, status: 403, data: { error: "denied" } });
  });
  await assert.rejects(plansApi.fetchPlans(API), /403/);
  assert.deepEqual(failedPaths, ["/api/mesh/relay-to"], "a peer refusal never falls back to local plans");

  clearRelayTarget();
  for (const body of [{ error: "Opaque refusal" }, { error: { message: "Opaque refusal", code: "denied" } }]) {
    globalThis.fetch = muxMockFetch(async () => Response.json(body, { status: 400 }));
    await assert.rejects(plansApi.runPlanControlLoop(API, "pl_peer"), { message: "Opaque refusal" });
  }
  globalThis.fetch = muxMockFetch(async () => new Response("proxy failure", { status: 502 }));
  await assert.rejects(plansApi.runPlanControlLoop(API, "pl_peer"), { message: "The Curator round could not start (HTTP 502)." });
  globalThis.fetch = muxMockFetch(async (input) => {
    assert.equal(String(input), "http://gateway:7685/api/plans?view=summary");
    return Response.json({ plans: [{ ...FIXTURE_PLAN, revision: "1" }] });
  });
  assert.deepEqual(await plansApi.fetchPlans(API), [{ ...FIXTURE_PLAN, revision: "1" }]);
  globalThis.fetch = muxMockFetch(async (input) => {
    const url = new URL(String(input));
    assert.equal(url.pathname, "/api/plans");
    assert.equal(url.searchParams.get("chat"), "1");
    assert.equal(url.searchParams.has("full"), false);
    assert.equal(url.searchParams.get("query"), "COD-12");
    assert.equal(url.searchParams.get("team"), "team-a");
    return Response.json({ plans: [] });
  });
  await plansApi.fetchPlans(API, undefined, { chat: true, query: "COD-12", project: "/repo", team: "team-a" });
  const windowRequests: URL[] = [];
  let changeRevision = false;
  globalThis.fetch = muxMockFetch(async (input) => {
    const url = new URL(String(input)); windowRequests.push(url);
    const second = url.searchParams.has("after_position");
    return Response.json({ ...remotePlan, revision: second && changeRevision ? "2" : "1",
      tasks: [{ id: second ? "second" : "first" }],
      task_window: { next_cursor: second ? null : { position: 1, task_id: "first" }, total: 2, cancelled: 0 } });
  });
  const requested = await plansApi.fetchRequestedPlanTaskMetadata(API, remotePlan.id,"blocked");
  assert.deepEqual(requested.tasks.map((task) => task.id),["first","second"]);
  assert(windowRequests.every((url) => url.searchParams.get("view") === "task_metadata" && url.searchParams.get("page_size") === "25" && url.searchParams.get("status") === "blocked" && !url.searchParams.has("full")));
  changeRevision = true;
  await assert.rejects(plansApi.fetchRequestedPlanTaskMetadata(API,remotePlan.id),/Tasks changed/);
  console.log("plansApi peer routing passed");
}

void main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
