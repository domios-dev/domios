import assert from "node:assert/strict";
import { act } from "react";
import { test } from "vitest";
import { GeneralInventorySections } from "./GeneralInventory";
import { useGeneralJobs } from "./useGeneralJobs";
import { cronSelectionStore } from "../Cron/cronSelectionStore";
import { SidebarCronRow } from "./SidebarResourceRow";
import { __setInvokeForTest } from "../../lib/bridge";
import { graphA, installBridge, machine, mount, named, settle, teamCron, workspaces as jobsWorkspaces } from "../Jobs/jobsEntry.testdata";
import { daemon as remoteDaemon, schedule, shell, tabs, teams, workspaces } from "../General/generalOverview.testdata";
import { mergeJobs, NO_JOBS_FILTER } from "../Jobs/jobsModel";

const daemon = { ...remoteDaemon, source_peer_id: undefined, source_peer: undefined };

test("General sidebar disclosures persist, navigate to original shells and open job details and service refusals", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  cronSelectionStore.close();
  let shownAll = 0;
  let opened = 0;
  const unsubscribe = cronSelectionStore.subscribe(() => { opened += 1; });
  const navigated: string[][] = [];
  const props = {
    onShowAllJobs: () => { shownAll += 1; },
    generalWorkspaceId: workspaces[0].id, workspaces, teams, tabs, touchMode: true, onCloseTab: () => {}, onRenameTab: () => {},
    onNavigate: (workspaceId: string, tabId: string) => navigated.push([workspaceId, tabId]),
    jobs: mergeJobs(NO_JOBS_FILTER, [{ ...teamCron, workspace_id: workspaces[0].id }], []),
    inventory: { services: [daemon, schedule], peers: [], thisMachine: { name: "Local", ip: "127.0.0.1" }, loading: false, error: null, loaded: true, refresh: () => {} },
  };
  const view = await mount(<GeneralInventorySections {...props} />);
  const toggle = (name: string) => view.host.querySelector<HTMLButtonElement>(`section[aria-label="${name}"] button[aria-expanded]`)!;
  try {
    for (const name of ["Jobs", "Services", "Utility shells"]) {
      assert.equal(toggle(name).getAttribute("aria-expanded"), "false");
      await act(async () => toggle(name).click());
      assert.equal(toggle(name).getAttribute("aria-expanded"), "true");
      assert.equal(view.host.querySelector(`ul[aria-label="${name}"]`)?.hasAttribute("hidden"), false);
    }
    const jobsList = view.host.querySelector('ul[aria-label="Jobs"]')!;
    assert.equal(jobsList.lastElementChild?.querySelector("button")?.getAttribute("aria-label"), "Show all jobs");
    await act(async () => named(jobsList, "Show all jobs")!.click());
    assert.equal(shownAll, 1);
    await act(async () => toggle("Jobs").click());
    assert.equal(named(view.host, "Show all jobs"), null);
    await act(async () => toggle("Jobs").click());
    await act(async () => view.root.render(<GeneralInventorySections {...props} jobsFailed inventory={{ ...props.inventory, error: "transient" }} />));
    assert.equal(view.host.querySelector('[role="alert"]'), null);
    assert.ok(named(view.host, `Open job ${teamCron.name}`));
    assert.equal(toggle("Jobs").getAttribute("aria-label"), "JOBS 1, 1 working");
    await act(async () => named(view.host, `Open job ${teamCron.name}`)!.click());
    assert.equal(opened, 1);
    assert.deepEqual(cronSelectionStore.get(), { workspaceId: workspaces[0].id, cronId: teamCron.id });
    await act(async () => named(view.host, `Open tab qa shell, ${shell.title}`)!.click());
    assert.deepEqual(navigated, [[workspaces[0].id, shell.id]]);
    await act(async () => named(view.host, `Open ${daemon.name}, machine ${daemon.machine_name}`)!.click());
    const dialog = document.querySelector('[role="dialog"]');
    assert.ok(dialog);
    const remove = Array.from(dialog.querySelectorAll("button")).find((button) => button.textContent?.trim() === "Remove registry entry");
    assert.ok(remove?.disabled);
    await act(async () => view.root.render(<GeneralInventorySections {...props} key="remount" />));
    assert.equal(toggle("Services").getAttribute("aria-expanded"), "true");
    await act(async () => toggle("Services").click());
    assert.equal(toggle("Services").getAttribute("aria-expanded"), "false");
    await act(async () => view.root.render(<GeneralInventorySections {...props} key="empty" jobs={[]} inventory={{ ...props.inventory, services: [] }} tabs={{}} />));
    for (const name of ["Jobs", "Services", "Utility shells"]) {
      assert.equal(view.host.querySelector(`section[aria-label="${name}"]`), null);
    }
  } finally {
    await view.close();
    unsubscribe();
    cronSelectionStore.close();
  }
});

test("General jobs read only their workspace crons and graphs", async () => {
  const cleanup = await installBridge();
  machine.graphs.project = { ...graphA, workspace_id: "project" };
  let jobs: ReturnType<typeof useGeneralJobs> | undefined;
  function Probe() {
    jobs = useGeneralJobs([...jobsWorkspaces, ...workspaces], jobsWorkspaces[0].id);
    return null;
  }
  const view = await mount(<Probe />);
  try {
    await settle(() => !!jobs?.jobs.some((job) => job.kind === "cron") && !!jobs?.jobs.some((job) => job.kind === "sentinel"));
    assert.ok(machine.cronReads.some((read) => JSON.stringify(read) === JSON.stringify({ workspaceId: jobsWorkspaces[0].id })));
    assert.equal(jobs?.failed, false);
  } finally {
    await view.close();
    cleanup();
  }
});

test("shared Cron actions delete once and refresh their source", async () => {
  const deleted: unknown[] = [];
  let refreshed = 0;
  __setInvokeForTest(async (command, args) => {
    assert.equal(command, "cron_delete");
    deleted.push(args);
    return undefined as never;
  });
  const view = await mount(<SidebarCronRow job={teamCron} tabs={{}} onDeleted={() => { refreshed += 1; }} />);
  try {
    await act(async () => named(view.host, `Cron actions for ${teamCron.name}`)!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    const remove = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).find((button) => button.textContent?.includes("Delete cron"));
    assert.ok(remove);
    await act(async () => remove.click());
    assert.deepEqual(deleted, [{ id: teamCron.id }]);
    assert.equal(refreshed, 1);
  } finally {
    await view.close();
    __setInvokeForTest(null);
  }
});


test("system Cron row toggles once and renders deletion unavailable", async () => {
  const calls: unknown[] = [];
  let refreshed = 0;
  __setInvokeForTest(async (command, args) => {
    calls.push([command, args]);
    return undefined as never;
  });
  const view = await mount(<SidebarCronRow job={{ ...teamCron, protected: true }} tabs={{}} onDeleted={() => { refreshed += 1; }} />);
  try {
    for (const enabled of [true, false]) {
      await act(async () => view.root.render(<SidebarCronRow job={{ ...teamCron, enabled, protected: true }} tabs={{}} onDeleted={() => { refreshed += 1; }} />));
      await act(async () => named(view.host, `Cron actions for ${teamCron.name}`)!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
      const items = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'));
      const toggle = items.find((button) => button.textContent?.trim() === (enabled ? "Disable" : "Enable"));
      const unavailable = items.find((button) => button.textContent?.trim() === "System job cannot be deleted");
      assert.ok(toggle);
      assert.ok(unavailable?.disabled);
      assert.equal(items.some((button) => button.textContent?.trim() === "Delete cron"), false);
      await act(async () => { unavailable.click(); toggle.click(); });
    }
    assert.deepEqual(calls, [
      ["cron_toggle", { id: teamCron.id, enabled: false }],
      ["cron_toggle", { id: teamCron.id, enabled: true }],
    ]);
    assert.equal(refreshed, 2);
  } finally {
    await view.close();
    __setInvokeForTest(null);
  }
});

test("General hides failed empty groups even when Show all is available", async () => {
  let opened = 0;
  const view = await mount(<GeneralInventorySections generalWorkspaceId="general" workspaces={workspaces} teams={teams} tabs={{}}
    inventory={{ services: [], peers: [], thisMachine: { name: "Local", ip: "" }, loading: false, error: "transient", loaded: true, refresh: () => {} }} jobs={[]} jobsFailed
    onCloseTab={() => {}} onRenameTab={() => {}} onNavigate={() => {}} onShowAllJobs={() => { opened++; }} />);
  try {
    const showAll = view.host.querySelector<HTMLButtonElement>('section[aria-label="Jobs"] button[aria-label="Show all jobs"]');
    assert.equal(showAll, null);
    assert.equal(view.host.querySelector("section"), null);
    assert.equal(view.host.querySelector('[role="alert"]'), null);
    assert.equal(opened, 0);
  } finally { await view.close(); }
});
