import assert from "node:assert/strict";
import { orchestrationTestClient } from "../../lib/transport/orchestration.testdata";
import { readPeerJobs } from "./peerJobsData";
import { fixtureGraph } from "../Automation/fixtures.testdata";

async function main() {
  const calls: string[] = [];
  const client = orchestrationTestClient(async <T,>(_method: string, path: string) => {
    calls.push(path);
    if (path === "/api/mux/state") return { workspaces: [{ id: "remote-general", name: "General" }, { id: "remote-project", name: "Project" }], teams: [] } as T;
    if (path === "/api/crons") return [{ id: "remote-cron", name: "Backup", workspace_id: "remote-general", enabled: true }] as T;
    if (path === "/api/mesh/inventory") return { resources: [] } as T;
    const id = new URL(path, "http://fixture").searchParams.get("workspace_id");
    return { ...fixtureGraph(), workspace_id: id, scenarios: [], nodes: [], edges: [] } as T;
  });
  const data = await readPeerJobs(client, new AbortController().signal);
  assert.deepEqual(data.workspaces.map((workspace) => workspace.id), ["remote-general", "remote-project"]);
  assert.deepEqual(data.jobs.map((job) => job.title), ["Backup"]);
  assert.equal(data.partial, false);
  assert.ok(calls.includes("/api/automation/graph/workspace?workspace_id=remote-general"));
  assert.ok(calls.includes("/api/automation/graph/workspace?workspace_id=remote-project"));
  assert.equal(calls.filter((path) => path === "/api/crons").length, 1);
  const failedGraphs = orchestrationTestClient(async <T,>(method: string, path: string) => {
    if (path.startsWith("/api/automation/graph")) throw new Error("peer failure");
    return client.json<T>(method as "GET", path);
  });
  const partial = await readPeerJobs(failedGraphs, new AbortController().signal);
  assert.equal(partial.partial, true);
  assert.equal(partial.jobs[0].title, "Backup");
  await assert.rejects(readPeerJobs(orchestrationTestClient(async () => { throw new Error("offline"); }), new AbortController().signal));
  console.log("9 peer Jobs transport assertions passed");
}
void main();
