// Run: npx tsx src/components/Jobs/JobsFilters.test.tsx
import assert from "node:assert/strict";
import { act, useState } from "react";
import { foreignCron, graphA, graphB, installBridge, installDom, machine, machineTransport, mount, named, pipelineFixture, remoteTeam, removeCapsules, renderedJobs, runCases, scenarioA, scenarioB, settle, teamA, teamCron, teams, unteamedCron, workspaceA, workspaceB, workspaces } from "./jobsEntry.testdata";

async function main() {
  const win = installDom();
  const cleanup = await installBridge();
  const [{ JobsView }, { WorkspaceNavigationActions }] = await Promise.all([
    import("./JobsView"),
    import("../Sidebar/WorkspaceNavigationActions"),
  ]);

  async function pickFromMenu(host: HTMLElement, trigger: string, item: string) {
    const button = named(host, trigger);
    assert.ok(button, `${trigger} rendered`);
    await act(async () => button.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true })));
    await settle(() => document.querySelector(`[role="menu"][aria-label="${trigger}"]`) !== null);
    const option = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).find((candidate) => candidate.textContent?.includes(item));
    assert.ok(option, `${item} offered by ${trigger}`);
    await act(async () => option.click());
  }

  const tests: Array<[string, () => Promise<void>]> = [];
  function test(name: string, run: () => Promise<void>) { tests.push([name, run]); }

  test("the workspace Jobs action opens the view with only that workspace's capsule and every Team's jobs", async () => {
    const transport = machineTransport();
    function Harness() {
      const [open, setOpen] = useState<string | null>(null);
      return <>
        <WorkspaceNavigationActions workspaceId={workspaceA} workspaceName="Workspace A" onOpenJobs={setOpen} onOpenMap={() => {}} />
        {open && <JobsView workspaces={workspaces} teams={teams} initialFilter={{ workspaceId: open, teamId: null }} adapter={transport} onClose={() => setOpen(null)} />}
      </>;
    }
    const view = await mount(<Harness />);
    try {
      await act(async () => named(view.host, "Jobs")!.click());
      await settle(() => renderedJobs(view.host).length === 3);
      assert.deepEqual(removeCapsules(view.host).map((capsule) => capsule.getAttribute("aria-label")), ["Remove filter: Workspace Workspace A"]);
      assert.deepEqual(renderedJobs(view.host), [`cron:${teamCron.id}`, `cron:${unteamedCron.id}`, scenarioA].sort());
      assert.deepEqual(machine.cronReads[machine.cronReads.length - 1], { workspaceId: workspaceA }, "Cron records are read for the exact workspace");
      assert.ok(transport.attachments.every((scope) => scope.workspace_id === workspaceA), "only the filtered workspace graph is attached");
      await act(async () => named(view.host, "Back to where you were")!.click());
      assert.equal(view.host.querySelector('section[aria-label="Jobs"]'), null);
    } finally { await view.close(); }
  });

  test("adding and removing capsules changes the rendered job set and keeps focus on the filter controls", async () => {
    const view = await mount(<JobsView workspaces={workspaces} teams={teams} initialFilter={{ workspaceId: workspaceA, teamId: null }} adapter={machineTransport()} />);
    try {
      await settle(() => renderedJobs(view.host).length === 3);
      await pickFromMenu(view.host, "Add Team filter", "Team A");
      assert.ok(!Array.from(document.querySelectorAll('[role="menuitem"]')).some((item) => item.textContent?.includes(remoteTeam.name!)), "remote Teams are not offered");
      await settle(() => renderedJobs(view.host).length === 2);
      assert.deepEqual(renderedJobs(view.host), [`cron:${teamCron.id}`, scenarioA].sort());
      assert.equal(document.activeElement, named(view.host, "Remove filter: Team Team A"), "the new capsule takes focus");

      await act(async () => named(view.host, "Remove filter: Team Team A")!.click());
      await settle(() => renderedJobs(view.host).length === 3);
      assert.equal(document.activeElement, named(view.host, "Add Team filter"));

      await act(async () => named(view.host, "Remove filter: Workspace Workspace A")!.click());
      await settle(() => renderedJobs(view.host).length === 5);
      assert.deepEqual(renderedJobs(view.host), [`cron:${teamCron.id}`, `cron:${unteamedCron.id}`, `cron:${foreignCron.id}`, scenarioA, scenarioB].sort(), "no capsules shows every job on this machine");
      assert.deepEqual(machine.cronReads[machine.cronReads.length - 1], {}, "no workspace capsule reads every Cron");
      assert.equal(removeCapsules(view.host).length, 0);
      assert.equal(document.activeElement, named(view.host, "Add workspace filter"));

      await pickFromMenu(view.host, "Add workspace filter", "Workspace B");
      await settle(() => renderedJobs(view.host).length === 2);
      assert.deepEqual(renderedJobs(view.host), [`cron:${foreignCron.id}`, scenarioB].sort());
      assert.equal(named(view.host, "Add Team filter"), null, "a workspace without Teams offers no Team capsule");
    } finally { await view.close(); }
  });

  test("a Team capsule alone narrows every workspace to that Team, and an unknown Team stays unavailable", async () => {
    const view = await mount(<JobsView workspaces={workspaces} teams={teams} initialFilter={{ workspaceId: null, teamId: teamA.id }} adapter={machineTransport()} />);
    try {
      await settle(() => renderedJobs(view.host).length === 2);
      assert.deepEqual(renderedJobs(view.host), [`cron:${teamCron.id}`, scenarioA].sort());
      await act(async () => view.root.render(<JobsView workspaces={workspaces} teams={[remoteTeam]} initialFilter={{ workspaceId: null, teamId: teamA.id }} adapter={machineTransport()} />));
      assert.ok(view.host.querySelector('[role="alert"]'), "a removed Team is reported, not shown as empty");
      assert.equal(view.host.querySelector('ul[aria-label="Jobs"]'), null);
      assert.ok(named(view.host, "Remove filter: Team Unavailable Team"), "the stale capsule stays removable");
    } finally { await view.close(); }
  });

  test("an empty filter announces a centered status and removing the capsule reveals jobs elsewhere", async () => {
    machine.crons = [foreignCron];
    machine.graphs = { [workspaceA]: { ...graphA, scenarios: [] }, [workspaceB]: graphB };
    const view = await mount(<JobsView workspaces={workspaces} teams={teams} initialFilter={{ workspaceId: workspaceA, teamId: null }} adapter={machineTransport()} />);
    try {
      await settle(() => view.host.querySelector('[role="status"][aria-label="No jobs"]') !== null);
      assert.equal(view.host.querySelector('ul[aria-label="Jobs"]'), null);
      await act(async () => named(view.host, "Remove filter: Workspace Workspace A")!.click());
      await settle(() => renderedJobs(view.host).length === 2);
      assert.equal(view.host.querySelector('[role="status"][aria-label="No jobs"]'), null);
    } finally {
      await view.close();
      machine.reset();
    }
  });

  test("pipeline rows follow workspace and Team capsules and count once for Clear stopped", async () => {
    machine.graphs = {
      [workspaceA]: { ...graphA, blueprints: [pipelineFixture(graphA.scenarios[0].id, teamA.id), pipelineFixture("tab-pipeline", null)] },
      [workspaceB]: { ...graphB, blueprints: [pipelineFixture("foreign-pipeline", null)] },
    };
    const view = await mount(<JobsView workspaces={workspaces} teams={teams} initialFilter={{ workspaceId: workspaceA, teamId: teamA.id }} adapter={machineTransport()} />);
    try {
      await settle(() => renderedJobs(view.host).length === 2);
      assert.deepEqual(renderedJobs(view.host), [`blueprint:${graphA.scenarios[0].id}`, `cron:${teamCron.id}`].sort());
      assert.ok(named(view.host, "Clear stopped, 1 jobs"));
      await act(async () => named(view.host, "Remove filter: Team Team A")!.click());
      await settle(() => renderedJobs(view.host).length === 4);
      assert.ok(named(view.host, "Clear stopped, 2 jobs"));
      assert.ok(renderedJobs(view.host).includes("blueprint:tab-pipeline"));
      assert.ok(!renderedJobs(view.host).includes("blueprint:foreign-pipeline"));
      await act(async () => named(view.host, "Remove filter: Workspace Workspace A")!.click());
      await settle(() => renderedJobs(view.host).length === 7);
      assert.ok(renderedJobs(view.host).includes("blueprint:foreign-pipeline"));
    } finally { await view.close(); machine.reset(); }
  });

  test("a peer reset detaches the listed records and the rebind attaches the next connection", async () => {
    const transport = machineTransport();
    const view = await mount(<JobsView workspaces={workspaces} teams={teams} initialFilter={{ workspaceId: workspaceA, teamId: null }} adapter={transport} />);
    try {
      await settle(() => renderedJobs(view.host).length === 3);
      await act(async () => { win.dispatchEvent(new win.CustomEvent("codeterm-peer-switch-reset", { detail: { switchId: 1 } })); });
      assert.equal(renderedJobs(view.host).length, 0, "records from the old connection are gone");
      await act(async () => { win.dispatchEvent(new win.CustomEvent("codeterm-peer-switch-rebind", { detail: { switchId: 1 } })); });
      await settle(() => renderedJobs(view.host).length === 3);
      assert.equal(transport.attachments[transport.attachments.length - 1]?.connection_id, "host:1");
    } finally { await view.close(); }
  });

  await runCases("Jobs filter", tests, cleanup);
}

void main().catch((error) => { console.error(error); process.exitCode = 1; });
