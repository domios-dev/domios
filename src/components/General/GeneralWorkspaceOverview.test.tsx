import assert from "node:assert/strict";
import { act } from "react";
import { test } from "vitest";
import { GeneralOverviewRail } from "./GeneralWorkspaceOverview";
import { graphA, installBridge, installDom, machine, machineTransport, mount, named, settle, workspaces as jobsWorkspaces } from "../Jobs/jobsEntry.testdata";
import { daemon, schedule, shell, tabs, teams, workspaces } from "./generalOverview.testdata";

async function main() {
  installDom();
  const cleanup = await installBridge();
  machine.graphs.project = { ...graphA, workspace_id: "project" };
  let refreshes = 0;
  const navigated: string[][] = [];
  const view = await mount(<GeneralOverviewRail
    workspaces={[...workspaces, ...jobsWorkspaces.map((workspace) => ({ ...workspace, tabs: [], active_tab_id: null, cwd: null }))]}
    teams={teams} tabs={tabs} onNavigate={(workspaceId, tabId) => navigated.push([workspaceId, tabId])}
    jobsProps={{ adapter: machineTransport() }}
    inventory={{ services: [daemon, schedule], peers: [], thisMachine: { name: "Local", ip: "127.0.0.1" }, loading: false, error: null, refresh: () => { refreshes++; } }}
  />);
  try {
    await settle(() => machine.cronReads.length > 0);
    assert.ok(machine.cronReads.some((read) => JSON.stringify(read) === "{}"), "Jobs mounts at machine scope");
    assert.ok(view.host.querySelector('section[aria-label="Jobs"]'));
    const services = view.host.querySelector('section[aria-label="Services"]');
    const schedules = view.host.querySelector('section[aria-label="Schedules"]');
    assert.ok(services && schedules);
    assert.ok(named(services, `Open ${daemon.name}, machine ${daemon.machine_name}`), "daemon inventory renders in Services");
    assert.equal(named(services, `Open ${schedule.name}, machine ${schedule.machine_name}`), null);
    assert.ok(named(schedules, `Open ${schedule.name}, machine ${schedule.machine_name}`), "remote schedule inventory has a home");
    console.log("✓ General rail mounts machine Jobs, daemon inventory and remote schedules");
    const navigate = named(view.host, `Open utility shell ${shell.title}`);
    assert.ok(navigate);
    await act(async () => navigate.click());
    assert.deepEqual(navigated, [[workspaces[0].id, shell.id]]);
    console.log("✓ Utility shell navigation fires once with its original workspace and tab");
    const refresh = Array.from(view.host.querySelectorAll('button')).find((button) => button.textContent === "Refresh inventory");
    assert.ok(refresh);
    await act(async () => refresh.click());
    assert.equal(refreshes, 1);
    console.log("✓ Inventory refresh fires its existing adapter once");
    const openDaemon = named(services, `Open ${daemon.name}, machine ${daemon.machine_name}`);
    assert.ok(openDaemon);
    await act(async () => openDaemon.click());
    const dialog = view.host.querySelector('[role="dialog"]');
    assert.ok(dialog, "service inventory opens the existing drawer");
    const remove = Array.from(dialog.querySelectorAll('button')).find((button) => button.textContent?.trim() === "Remove registry entry");
    assert.ok(remove?.disabled, "offline capability refuses registry removal");
    console.log("✓ Offline service inspection preserves the existing capability refusal");
  } finally {
    await act(async () => view.root.unmount());
    cleanup();
  }
}
test("General rail mounts machine Jobs, inventory, capability refusals and utility navigation", main);
