import assert from "node:assert/strict";
import { Window } from "happy-dom";
import type { ApiConfig } from "../../lib/types";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";

const win = new Window({ url: "http://gateway:7685" });
Object.assign(globalThis, { window: win, localStorage: win.localStorage, sessionStorage: win.sessionStorage });
const { setRelayTarget, clearRelayTarget, setTokenFor, invalidateWebRequestContext } = await import("../../lib/webApi");
const plansApi = await import("./plansApi");
const { fetchPlanSkills, fetchPlanSkill } = await import("./planView/planSkillsApi");
const { fetchPlanBoardState } = await import("./missionControl/missionControlState");
const API: ApiConfig = { enabled: true, host: "peer", port: 7685, token: "unused-peer-token", cors: true };
setTokenFor("http://gateway:7685", "gateway-token");

type Request = { method: string; path: string; body: unknown; target_peer_id?: string };
const requests: Request[] = [];
const remotePlan = { ...FIXTURE_PLAN, id: "pl_peer", project_path: "/home/peer/project" };
const index = { plan_id: remotePlan.id, title: remotePlan.title };
const skill = { slug: "check", title: "Check", body: "opaque", ledger: [] };

globalThis.fetch = async (input, init) => {
  const url = new URL(String(input));
  assert.equal(new Headers(init?.headers).get("Authorization"), "Bearer gateway-token");
  if (url.pathname !== "/api/mesh/relay-to") return Response.json({ plans: [] });
  const request = JSON.parse(String(init?.body)) as Request;
  requests.push(request);
  let data: unknown = remotePlan;
  if (request.path.startsWith("/api/plans?")) data = { plans: request.path.includes("full=1") ? [remotePlan] : [index] };
  if (request.path === "/api/plans") data = { plans: [index] };
  if (request.path.endsWith("/reconcile")) data = { drift: [{ id: "peer-drift" }] };
  if (request.path === "/api/plans/resolve-refs") data = { refs: [{ plan_id: remotePlan.id }] };
  if (request.path.startsWith("/api/specs/")) data = { spec: { sections: [{ id: "peer-section" }] } };
  if (request.path.endsWith("/skills")) data = { skills: [skill] };
  if (request.path.endsWith("/skills/check")) data = { skill };
  return Response.json({ ok: true, status: 200, data, error: null });
};

setRelayTarget("ubunta", "ubunta");
assert.deepEqual((await plansApi.fetchPlans(API)).map((plan) => plan.id), [remotePlan.id]);
assert.deepEqual((await plansApi.fetchPlanIndex(API)).map((plan) => plan.plan_id), [remotePlan.id]);
assert.equal((await plansApi.fetchPlan(API, remotePlan.id)).id, remotePlan.id);
assert.deepEqual(await plansApi.fetchSpecSections(API, "spec/peer"), [{ id: "peer-section" }]);
assert.deepEqual(await plansApi.fetchReconcile(API, remotePlan.id), [{ id: "peer-drift" }]);
assert.deepEqual(await plansApi.resolvePlanRefs(API, [{ plan_id: remotePlan.id, target_kind: "plan" }]), [{ plan_id: remotePlan.id }]);
assert.deepEqual(await plansApi.fetchUnownedPlanIndex(API), [index]);
assert.deepEqual(await fetchPlanSkills(API, remotePlan.id), [skill]);
assert.deepEqual(await fetchPlanSkill(API, remotePlan.id, skill.slug), skill);
await fetchPlanBoardState(API, remotePlan.id);
await plansApi.fetchPlans(API, undefined, { project: remotePlan.project_path });
await plansApi.renamePlan(API, remotePlan.id, "Peer title");
assert.equal(requests.at(-1)?.method, "PATCH");
assert.deepEqual(requests.at(-1)?.body, { title: "Peer title" });
assert(requests.some((request) => request.path === "/api/plans?full=1&project=%2Fhome%2Fpeer%2Fproject"));
assert(requests.some((request) => request.path === "/api/specs/spec%2Fpeer"));
assert(requests.every((request) => request.target_peer_id === "ubunta"));

const ctrl = new AbortController();
ctrl.abort();
await assert.rejects(plansApi.fetchPlans(API, ctrl.signal), { name: "AbortError" });

let finishOld!: (value: Response) => void;
globalThis.fetch = () => new Promise<Response>((resolve) => { finishOld = resolve; });
const oldRead = plansApi.fetchPlans(API);
invalidateWebRequestContext();
setRelayTarget("eight", "eight");
finishOld(Response.json({ ok: true, status: 200, data: { plans: [remotePlan] } }));
await assert.rejects(oldRead, { name: "AbortError" });

clearRelayTarget();
globalThis.fetch = async (input) => {
  assert.equal(String(input), "http://gateway:7685/api/plans?full=1");
  return Response.json({ plans: [FIXTURE_PLAN] });
};
assert.deepEqual(await plansApi.fetchPlans(API), [FIXTURE_PLAN]);
console.log("plansApi peer routing passed");
