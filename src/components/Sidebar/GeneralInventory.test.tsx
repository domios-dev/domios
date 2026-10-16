import assert from "node:assert/strict";
import { act } from "react";
import { test } from "vitest";
import { GeneralInventorySections } from "./GeneralInventory";
import { useGeneralJobs } from "./useGeneralJobs";
import { cronSelectionStore } from "../Cron/cronSelectionStore";
import { SidebarCronRow } from "./SidebarResourceRow";
import { __setInvokeForTest } from "../../lib/bridge";
import { graphA, installBridge, machine, mount, named, settle, teamCron, workspaces as jobsWorkspaces } from "../Jobs/jobsEntry.testdata";
import { daemon, schedule, shell, tabs, teams, workspaces } from "../General/generalOverview.testdata";
import { mergeJobs, NO_JOBS_FILTER } from "../Jobs/jobsModel";

test("General sidebar disclosures persist, navigate to original shells and open job details and service refusals", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  cronSelectionStore.close();
  let opened = 0;
  const unsubscribe = cronSelectionStore.subscribe(() => { opened += 1; });
  const navigated: string[][] = [];
  const props = {
    workspaces, teams, tabs, touchMode: true, onCloseTab: () => {}, onRenameTab: () => {},
    onNavigate: (workspaceId: string, tabId: string) => navigated.push([workspaceId, tabId]),
    jobs: mergeJobs(NO_JOBS_FILTER, [teamCron], []),
    inventory: { services: [daemon, schedule], peers: [], thisMachine: { name: "Local", ip: "127.0.0.1" }, loading: false, error: null, refresh: () => {} },
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
    assert.equal(toggle("Jobs").getAttribute("aria-label"), "Jobs, 2");
    await act(async () => named(view.host, `Open job ${teamCron.name}`)!.click());
    assert.equal(opened, 1);
    assert.deepEqual(cronSelectionStore.get(), { workspaceId: teamCron.workspace_id, cronId: teamCron.id });
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
      assert.equal(toggle(name).getAttribute("aria-label"), `${name}, 0`);
      assert.equal(toggle(name).getAttribute("aria-expanded"), "false");
    }
  } finally {
    await view.close();
    unsubscribe();
    cronSelectionStore.close();
  }
});

test("General jobs read machine crons and workspace graphs through existing jobs sources", async () => {
  const cleanup = await installBridge();
  machine.graphs.project = { ...graphA, workspace_id: "project" };
  let jobs: ReturnType<typeof useGeneralJobs> | undefined;
  function Probe() {
    jobs = useGeneralJobs([...jobsWorkspaces, ...workspaces]);
    return null;
  }
  const view = await mount(<Probe />);
  try {
    await settle(() => !!jobs?.jobs.some((job) => job.kind === "cron") && !!jobs?.jobs.some((job) => job.kind === "sentinel"));
    assert.ok(machine.cronReads.some((read) => JSON.stringify(read) === "{}"));
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
