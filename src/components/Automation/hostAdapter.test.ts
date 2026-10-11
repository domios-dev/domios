import assert from "node:assert/strict";
import { configureOrchestrationTransport } from "../../lib/transport/orchestrationRuntime";
import { orchestrationTestClient } from "../../lib/transport/orchestration.testdata";
import { hostAutomationAdapter, AUTOMATION_GRAPH_EVENT } from "./hostAdapter";
import { attachment, deferred, fixtureGraph, sentinelIdentity } from "./fixtures.testdata";
import { ALL_JOBS_SCOPE, type AutomationGraph, type AutomationScope } from "./types";

async function main() {
  const calls: Array<{ method: string; path: string; body?: unknown }> = [];
  let event!: (payload: AutomationScope & { revision: string }) => void;
  let off = false;
  configureOrchestrationTransport(orchestrationTestClient(async <T,>(method: string, path: string, body?: unknown) => {
    calls.push({ method, path, body });
    return fixtureGraph() as T;
  }, (stream, handler) => {
    assert.equal(stream, "/api/events/stream");
    event = (payload) => handler(AUTOMATION_GRAPH_EVENT, payload);
    return () => { off = true; };
  }));
  {
    assert.deepEqual(await hostAutomationAdapter.read(attachment, new AbortController().signal), fixtureGraph());
    assert.deepEqual(calls.pop(), { method: "GET", path: `/api/automation/graph?workspace_id=${attachment.workspace_id}&team_id=${attachment.team_id}`, body: undefined });
    await hostAutomationAdapter.read({ ...attachment, team_id: null }, new AbortController().signal);
    assert(calls.pop()?.path.endsWith("&team_id="));
    await hostAutomationAdapter.read({ ...attachment, team_id: ALL_JOBS_SCOPE }, new AbortController().signal);
    assert.deepEqual(calls.pop(), { method: "GET", path: `/api/automation/graph/workspace?workspace_id=${attachment.workspace_id}`, body: undefined });
    console.log("✓ snapshots use exact Team scopes or one bounded workspace-wide read");

    let changed = 0;
    const unsubscribe = await hostAutomationAdapter.subscribe(attachment, () => { changed++; });
    event({ ...attachment, team_id: "other", revision: "foreign" });
    event({ ...attachment, workspace_id: "other", revision: "foreign" });
    assert.equal(changed, 0);
    event({ ...attachment, revision: "new" });
    assert.equal(changed, 1);
    unsubscribe();
    assert.equal(off, true);
    let allChanged = 0;
    const allUnsubscribe = await hostAutomationAdapter.subscribe({ ...attachment, team_id: ALL_JOBS_SCOPE }, () => { allChanged++; });
    event({ ...attachment, team_id: "other", revision: "team-b" });
    assert.equal(allChanged, 1);
    allUnsubscribe();
    console.log("✓ committed-change subscription filters exact scopes or any Team in the workspace");

    await hostAutomationAdapter.stop(attachment, sentinelIdentity);
    assert.deepEqual(calls.pop(), { method: "POST", path: `/api/automation/blueprints/${sentinelIdentity.id}/stop`, body: { workspace_id: attachment.workspace_id, team_id: attachment.team_id, caller_tab_id: undefined, session_binding_nonce: undefined } });
    await hostAutomationAdapter.delete({ ...attachment, team_id: ALL_JOBS_SCOPE }, { ...sentinelIdentity, team_id: "team-b" });
    assert.deepEqual(calls.pop(), { method: "DELETE", path: `/api/automation/blueprints/${sentinelIdentity.id}`, body: { workspace_id: attachment.workspace_id, team_id: "team-b" } });
    await assert.rejects(hostAutomationAdapter.delete(attachment, { ...sentinelIdentity, team_id: "other" }));
    await assert.rejects(hostAutomationAdapter.stop(attachment, { ...sentinelIdentity, team_id: "other" }));
    assert.equal(calls.length, 0);
    console.log("✓ stop and delete retain identity Team scope without creator or nonce impersonation");

    const held = deferred<AutomationGraph>();
    configureOrchestrationTransport(orchestrationTestClient(async <T,>() => held.promise as Promise<T>));
    const controller = new AbortController();
    const pending = hostAutomationAdapter.read(attachment, controller.signal);
    controller.abort();
    held.resolve(fixtureGraph());
    await assert.rejects(pending);
    console.log("✓ aborted attachment rejects a late HTTP snapshot");
    console.log("5 hostAutomationAdapter cases passed");
  }
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
