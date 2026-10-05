import assert from "node:assert/strict";
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Team } from "../../lib/types";
import { withFailureDeadline } from "../../test/withFailureDeadline";
import { attachment, deferred, fixtureGraph, FixtureTransport } from "./fixtures.testdata";
import type { AutomationGraph } from "./types";

const renderAct = (body: () => Promise<void>) => withFailureDeadline(act(body), "automation workspace state event");
async function main() {
  const win = new Window({ url: "http://gateway:7685" });
  for (const key of ["window", "document", "navigator", "localStorage", "sessionStorage", "HTMLElement", "Element", "Node", "Event", "KeyboardEvent", "CustomEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  const cronReads: unknown[] = [];
  __setInvokeForTest(async <T,>(command: string, args?: unknown): Promise<T> => {
    if (command !== "cron_list") throw new Error(`unexpected invoke: ${command}`);
    cronReads.push(args);
    return [] as T;
  });
  __setListenForTest(async () => () => undefined);
  const { WorkspaceAutomationView } = await import("./WorkspaceAutomationView");
  const { WorkspaceAutomationAction } = await import("./WorkspaceAutomationAction");
  const { ALL_JOBS } = await import("../Jobs/jobsModel");
  const team: Team = { id: attachment.team_id!, workspace_id: attachment.workspace_id, name: "Sentinel-only Team", lifecycle: "active", created_at: 0 };
  const foreign = { ...team, id: "foreign-team", name: "Foreign Team", workspace_id: "foreign-workspace" };
  const transport = new FixtureTransport();
  const sentinelOnly = { ...fixtureGraph(), nodes: [fixtureGraph().nodes[0]], edges: [] };
  transport.readOverride = async (scope) => scope.team_id === ALL_JOBS
    ? { ...sentinelOnly, team_id: null, all_teams: true }
    : scope.team_id === null ? { ...sentinelOnly, team_id: null, nodes: [] } : sentinelOnly;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let closed = 0;
  function Harness({ teams = [team, foreign] }: { teams?: Team[] }) {
    const [workspaceId, setWorkspaceId] = useState<string | null>(null);
    return <><WorkspaceAutomationAction workspaceId={attachment.workspace_id} workspaceName="Workspace A" onOpen={setWorkspaceId} />{workspaceId && <WorkspaceAutomationView workspaceId={workspaceId} teams={teams} adapter={transport} onClose={() => { closed++; setWorkspaceId(null); }} />}</>;
  }
  const selectTeam = async () => {
    const select = host.querySelector<HTMLSelectElement>('select[aria-label="Jobs Team"]');
    assert.ok(select);
    await renderAct(async () => { select.value = team.id; select.dispatchEvent(new Event("change", { bubbles: true })); });
  };
  try {
    await renderAct(async () => root.render(<Harness />));
    const action = host.querySelector<HTMLButtonElement>('[data-testid="workspace-automation-action"]');
    assert.ok(action);
    await renderAct(async () => { action.focus(); action.click(); });
    assert.ok(host.querySelector('section[aria-label="Jobs"]'));
    assert.deepEqual(cronReads[cronReads.length - 1], { workspaceId: attachment.workspace_id }, "Jobs reads Cron records for the exact workspace");
    const teamPicker = host.querySelector<HTMLSelectElement>('select[aria-label="Jobs Team"]');
    assert.ok(teamPicker);
    assert.equal(teamPicker.value, ALL_JOBS, "All jobs is selected by default");
    assert.equal(teamPicker.options[0].textContent, "All jobs");
    assert.ok(!host.textContent?.includes(foreign.name));
    assert.equal(transport.attachments[transport.attachments.length - 1].team_id, ALL_JOBS);
    await selectTeam();
    const scenario = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.includes(sentinelOnly.scenarios[0].name));
    assert.ok(scenario);
    await renderAct(async () => scenario.click());
    assert.ok(host.textContent?.includes(sentinelOnly.nodes[0].name), "Sentinel-only Team is reachable without cron/plan records");
    assert.equal(host.querySelectorAll('[data-edge-kind]').length, 0);
    const node = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.includes(sentinelOnly.nodes[0].name));
    assert.ok(node);
    await renderAct(async () => node.click());
    assert.ok(host.querySelector('aside[aria-label="Details: Watcher A"]'));
    assert.ok(host.textContent?.includes(sentinelOnly.nodes[0].purpose!));
    console.log("✓ workspace action reaches Sentinel-only Team and selectable authoritative detail");

    const held = deferred<AutomationGraph>();
    transport.readOverride = async () => held.promise;
    const oldEvent = transport.changed!;
    await renderAct(async () => { oldEvent(); });
    const oldReads = transport.reads;
    await renderAct(async () => { win.dispatchEvent(new win.CustomEvent("codeterm-peer-switch-reset", { detail: { switchId: 1 } })); });
    assert.equal(host.querySelector("aside"), null);
    assert.ok(!host.textContent?.includes(sentinelOnly.nodes[0].name));
    assert.equal(transport.reads, oldReads);
    transport.readOverride = async () => { throw new Error("peer-unavailable-payload"); };
    await renderAct(async () => { win.dispatchEvent(new win.CustomEvent("codeterm-peer-switch-rebind", { detail: { switchId: 1 } })); });
    assert.ok(host.querySelector('[role="alert"]'));
    assert.ok(!host.textContent?.includes("peer-unavailable-payload"));
    await renderAct(async () => { held.resolve(sentinelOnly); oldEvent(); });
    assert.equal(host.querySelector("aside"), null);
    assert.ok(!host.textContent?.includes(sentinelOnly.nodes[0].name));
    assert.equal(transport.reads, oldReads + 1, "old subscription cannot refresh the new attachment");
    assert.equal(transport.attachments[transport.attachments.length - 1].connection_id, "host:1");
    console.log("✓ peer reset detaches old records and failed rebind never restores local/late data");

    await renderAct(async () => root.render(<Harness teams={[foreign]} />));
    assert.ok(host.textContent?.includes("selected Team is unavailable"));
    assert.equal(host.querySelector('[aria-label="Automations"]'), null);
    const close = host.querySelector<HTMLButtonElement>('button[aria-label="Close jobs"]');
    assert.ok(close);
    await renderAct(async () => close.click());
    assert.equal(host.querySelector('section[aria-label="Jobs"]'), null);
    assert.equal(closed, 1);
    console.log("✓ removed Team stays unavailable and close returns to workspace action");
    console.log("3 WorkspaceAutomationView cases passed");
  } finally {
    await renderAct(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
