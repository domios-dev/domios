import assert from "node:assert/strict";
import { act } from "react";
import type { PairedPeer } from "../../lib/types";
import { installDom, installBridge, machineTransport, mount, renderedJobs, settle, teams, workspaces } from "./jobsEntry.testdata";
import { orchestrationTestClient } from "../../lib/transport/orchestration.testdata";
import { fixtureGraph } from "../Automation/fixtures.testdata";
import { THIS_MACHINE } from "./jobsMachineModel";

async function main() {
  installDom();
  const cleanup = await installBridge();
  const { JobsView } = await import("./JobsView");
  const peer = { instance_id: "remote", instance_name: "Remote", online: false } as PairedPeer;
  const peerReads: string[] = [];
  const client = orchestrationTestClient(async <T,>(_method: string, path: string) => {
    peerReads.push(path);
    if (path === "/api/mux/state") return { workspaces: [{ id: "remote-only", name: "Remote workspace" }], teams: [] } as T;
    if (path === "/api/crons") return [{ id: "remote-job", name: "Remote only job", workspace_id: "remote-only", enabled: true, schedule: "0 0 * * *" }] as T;
    if (path === "/api/mesh/inventory") return { resources: [] } as T;
    return { ...fixtureGraph(), workspace_id: "remote-only", scenarios: [], nodes: [], edges: [] } as T;
  });
  const clientForPeer = (id: string) => { assert.equal(id, "online"); return client; };
  const view = await mount(<JobsView workspaces={workspaces} teams={teams} adapter={machineTransport()} machinePeers={[peer, { ...peer, instance_id: "online", instance_name: "Online", online: true }]} peerClient={clientForPeer} />);
  try {
    await settle(() => renderedJobs(view.host).length === 5);
    const select = () => view.host.querySelector<HTMLSelectElement>('select[aria-label="Machine"]')!;
    assert.equal(select().value, THIS_MACHINE);
    assert.deepEqual(Array.from(select().options).map((option) => option.value), [THIS_MACHINE, "online", "remote"]);
    await act(async () => { select().value = "remote"; select().dispatchEvent(new Event("change", { bubbles: true })); });
    assert.equal(select().value, "remote");
    assert.equal(renderedJobs(view.host).length, 0);
    assert.ok(Array.from(view.host.querySelectorAll('[role="status"]')).some((status) => status.textContent?.includes("Remote") && status.textContent.includes("unavailable")));
    assert.equal(view.host.querySelector('[aria-label="No jobs"]'), null);
    await act(async () => { select().value = "online"; select().dispatchEvent(new Event("change", { bubbles: true })); });
    await settle(() => renderedJobs(view.host).length === 1);
    assert.deepEqual(renderedJobs(view.host), ["cron:remote-job"]);
    assert.ok(peerReads.includes("/api/automation/graph/workspace?workspace_id=remote-only"));
    await act(async () => { select().value = THIS_MACHINE; select().dispatchEvent(new Event("change", { bubbles: true })); });
    await settle(() => renderedJobs(view.host).length === 5);
    console.log("Machine selector defaults to all local jobs, selects an unavailable peer honestly and returns to local jobs");
  } finally { await view.close(); cleanup(); }
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });
