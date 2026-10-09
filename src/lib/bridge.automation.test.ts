import assert from "node:assert/strict";
import { Window } from "happy-dom";

async function main() {
  const win = new Window({ url: "http://gateway:7685" });
  for (const [name, value] of Object.entries({ window: win, document: win.document, localStorage: win.localStorage, sessionStorage: win.sessionStorage })) {
    Object.defineProperty(globalThis, name, { configurable: true, value });
  }
  const { invoke } = await import("./bridge");
  const { setTokenFor, selfHost, setRelayTarget, clearRelayTarget } = await import("./webApi");
  setTokenFor(selfHost(), "fixture-token");
  const originalFetch = globalThis.fetch;
  const requests: Array<{ url: string; init?: RequestInit }> = [];
  globalThis.fetch = async (input, init) => {
    requests.push({ url: String(input), init });
    return Response.json({ marker: "fixture-response" });
  };
  try {
    const scope = { workspaceId: "workspace /?", teamId: "team &?" };
    assert.deepEqual(await invoke("automation_graph_snapshot", scope), { marker: "fixture-response" });
    const request = requests.pop()!;
    const url = new URL(request.url);
    assert.equal(url.pathname, "/api/automation/graph");
    assert.equal(url.searchParams.get("workspace_id"), scope.workspaceId);
    assert.equal(url.searchParams.get("team_id"), scope.teamId);
    assert.equal(request.init?.method, "GET");
    const headers = new Headers(request.init?.headers);
    assert.equal(headers.get("Authorization"), "Bearer fixture-token");
    assert.equal(headers.has("X-CodeTerm-Tab-Id"), false);
    console.log("✓ snapshot maps exact scope through established authenticated host-control");

    await invoke("automation_graph_snapshot", { workspaceId: "workspace-a", teamId: null });
    const unteamed = new URL(requests.pop()!.url);
    assert.equal(unteamed.searchParams.has("team_id"), true);
    assert.equal(unteamed.searchParams.get("team_id"), "");
    await assert.rejects(invoke("automation_graph_snapshot", { workspaceId: "workspace-a" }));
    await assert.rejects(invoke("automation_graph_snapshot", { teamId: null }));
    assert.equal(requests.length, 0);
    console.log("✓ unteamed is explicit and absent scope refuses before network");

    await invoke("automation_graph_workspace_snapshot", { workspaceId: "workspace all" });
    const allJobs = new URL(requests.pop()!.url);
    assert.equal(allJobs.pathname, "/api/automation/graph/workspace");
    assert.equal(allJobs.searchParams.get("workspace_id"), "workspace all");
    await assert.rejects(invoke("automation_graph_workspace_snapshot", {}));
    assert.equal(requests.length, 0);
    console.log("✓ All jobs uses a workspace-scoped snapshot endpoint");

    await invoke("automation_graph_stop_blueprint", { ...scope, id: "sentinel /?" });
    const stop = requests.pop()!;
    assert.equal(new URL(stop.url).pathname, "/api/automation/blueprints/sentinel%20%2F%3F/stop");
    assert.equal(stop.init?.method, "POST");
    assert.deepEqual(JSON.parse(String(stop.init?.body)), { workspace_id: scope.workspaceId, team_id: scope.teamId });
    assert.equal(new Headers(stop.init?.headers).has("X-CodeTerm-Tab-Id"), false);
    await assert.rejects(invoke("automation_graph_stop_blueprint", scope));
    assert.equal(requests.length, 0);
    console.log("✓ stop POST preserves scoped ID/body without creator impersonation");

    await invoke("automation_graph_delete_blueprint", { ...scope, id: "sentinel /?" });
    const deletion = requests.pop()!;
    assert.equal(new URL(deletion.url).pathname, "/api/automation/blueprints/sentinel%20%2F%3F");
    assert.equal(deletion.init?.method, "DELETE");
    assert.deepEqual(JSON.parse(String(deletion.init?.body)), { workspace_id: scope.workspaceId, team_id: scope.teamId });
    console.log("✓ Blueprint delete maps to the persisted host API");

    setRelayTarget("peer-fixture", "Peer fixture");
    globalThis.fetch = async (input, init) => {
      requests.push({ url: String(input), init });
      return Response.json({ ok: false, status: 403, data: { error: "peer-refused" } });
    };
    await assert.rejects(invoke("automation_graph_stop_blueprint", { ...scope, id: "sentinel-a" }));
    assert.equal(requests.length, 1);
    assert.equal(new URL(requests[0].url).pathname, "/api/mesh/relay-to");
    assert.deepEqual(JSON.parse(String(requests[0].init?.body)), { target_peer_id: "peer-fixture", method: "POST", path: "/api/automation/blueprints/sentinel-a/stop", body: { workspace_id: scope.workspaceId, team_id: scope.teamId } });
    console.log("✓ peer stop refusal remains unavailable without local fallback");
    await assert.rejects(invoke("automation_graph_delete_blueprint", { ...scope, id: "sentinel-a" }));
    assert.equal(requests.length, 2);
    assert.equal(new URL(requests[1].url).pathname, "/api/mesh/relay-to");
    const deletionRelay = JSON.parse(String(requests[1].init?.body));
    assert.equal(deletionRelay.method, "DELETE");
    assert.equal(deletionRelay.path, "/api/automation/blueprints/sentinel-a");
    console.log("✓ peer delete stays behind the host's authority refusal");
    console.log("6 bridge automation cases passed");
  } finally { globalThis.fetch = originalFetch; clearRelayTarget(); }
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
