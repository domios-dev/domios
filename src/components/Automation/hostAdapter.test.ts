import assert from "node:assert/strict";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { hostAutomationAdapter, AUTOMATION_GRAPH_EVENT } from "./hostAdapter";
import { attachment, deferred, fixtureGraph, sentinelIdentity } from "./fixtures.testdata";
import { ALL_JOBS_SCOPE, type AutomationGraph, type AutomationScope } from "./types";

async function main() {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return fixtureGraph() as T;
  });
  try {
    assert.deepEqual(await hostAutomationAdapter.read(attachment, new AbortController().signal), fixtureGraph());
    assert.deepEqual(calls.pop(), { command: "automation_graph_snapshot", args: { workspaceId: attachment.workspace_id, teamId: attachment.team_id } });
    await hostAutomationAdapter.read({ ...attachment, team_id: null }, new AbortController().signal);
    assert.equal(calls.pop()?.args?.teamId, null);
    await hostAutomationAdapter.read({ ...attachment, team_id: ALL_JOBS_SCOPE }, new AbortController().signal);
    assert.deepEqual(calls.pop(), { command: "automation_graph_workspace_snapshot", args: { workspaceId: attachment.workspace_id } });
    console.log("✓ snapshots use exact Team scopes or one bounded workspace-wide read");

    let event!: (payload: AutomationScope & { revision: string }) => void;
    let off = false;
    __setListenForTest(async <T,>(name: string, handler: (input: { payload: T }) => void) => {
      assert.equal(name, AUTOMATION_GRAPH_EVENT);
      event = (payload) => handler({ payload: payload as T });
      return () => { off = true; };
    });
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
    assert.deepEqual(calls.pop(), { command: "automation_graph_stop_blueprint", args: { workspaceId: attachment.workspace_id, teamId: attachment.team_id, id: sentinelIdentity.id } });
    await hostAutomationAdapter.delete({ ...attachment, team_id: ALL_JOBS_SCOPE }, { ...sentinelIdentity, team_id: "team-b" });
    assert.deepEqual(calls.pop(), { command: "automation_graph_delete_blueprint", args: { workspaceId: attachment.workspace_id, teamId: "team-b", id: sentinelIdentity.id } });
    await assert.rejects(hostAutomationAdapter.delete(attachment, { ...sentinelIdentity, team_id: "other" }));
    await assert.rejects(hostAutomationAdapter.stop(attachment, { ...sentinelIdentity, team_id: "other" }));
    assert.equal(calls.length, 0);
    console.log("✓ stop and delete retain identity Team scope without creator or nonce impersonation");

    const held = deferred<AutomationGraph>();
    __setInvokeForTest(async <T,>() => held.promise as Promise<T>);
    const controller = new AbortController();
    const pending = hostAutomationAdapter.read(attachment, controller.signal);
    controller.abort();
    held.resolve(fixtureGraph());
    await assert.rejects(pending);
    console.log("✓ aborted attachment rejects a late invoke snapshot");
    console.log("5 hostAutomationAdapter cases passed");
  } finally { __setInvokeForTest(null); __setListenForTest(null); }
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
