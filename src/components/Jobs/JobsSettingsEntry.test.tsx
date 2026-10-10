// Run: node scripts/run-ts-unit-tests.mjs src/components/Jobs/JobsSettingsEntry.test.tsx (SettingsPanel imports image assets)
import assert from "node:assert/strict";
import { act } from "react";
import type { AppConfig } from "../../lib/types";
import { entry, foreignCron, installBridge, installDom, machineTransport, mount, named, removeCapsules, renderedJobs, runCases, scenarioB, settle, teams, viewControls, workspaceA, workspaces } from "./jobsEntry.testdata";

async function main() {
  installDom();
  const cleanup = await installBridge();
  const [{ SettingsPanel }, { JobsView }, { jobsEntryFilter }] = await Promise.all([
    import("../Settings/SettingsPanel"),
    import("./JobsView"),
    import("./jobsLayout"),
  ]);

  const tests: Array<[string, () => Promise<void>]> = [];
  function test(name: string, run: () => Promise<void>) { tests.push([name, run]); }

  test("Settings > Jobs sits right before Logs and opens the same view with no capsules", async () => {
    const view = await mount(<SettingsPanel config={{ keybindings: {} } as AppConfig} onUpdateConfig={async () => {}} onClose={() => {}} workspaces={workspaces} teams={teams} />);
    try {
      const sections = Array.from(view.host.querySelectorAll<HTMLButtonElement>('[role="navigation"][aria-label="Settings sections"] button'));
      const jobsIndex = sections.findIndex((button) => button.textContent?.startsWith("Jobs"));
      assert.ok(jobsIndex >= 0, "Jobs section entry rendered");
      assert.ok(sections[jobsIndex + 1]?.textContent?.startsWith("Logs"), "Jobs precedes Logs");
      await act(async () => sections[jobsIndex].click());
      await settle(() => renderedJobs(view.host).length === 5);
      assert.ok(view.host.querySelector('[role="group"][aria-label="Job filters"]'));
      assert.equal(removeCapsules(view.host).length, 0, "the machine-wide entry starts unfiltered");
      assert.ok(entry(view.host, `cron:${foreignCron.id}`) && entry(view.host, scenarioB), "jobs of every workspace are listed");
      await act(async () => named(view.host, "Back to where you were")!.click());
      assert.equal(view.host.querySelector('section[aria-label="Jobs"]'), null, "closing Jobs returns to the Settings sections");
      assert.ok(view.host.querySelector('[role="navigation"][aria-label="Settings sections"]'));
    } finally { await view.close(); }
  });

  test("the workspace entry with its capsule removed renders exactly the Settings entry", async () => {
    const settings = await mount(<SettingsPanel config={{ keybindings: {} } as AppConfig} onUpdateConfig={async () => {}} onClose={() => {}} initialSection="jobs" workspaces={workspaces} teams={teams} />);
    const workspace = await mount(<JobsView workspaces={workspaces} teams={teams} initialFilter={jobsEntryFilter({ kind: "workspace", workspaceId: workspaceA })} adapter={machineTransport()} onClose={() => {}} />);
    try {
      await settle(() => renderedJobs(settings.host).length === 5);
      await settle(() => renderedJobs(workspace.host).length === 3);
      assert.notDeepEqual(viewControls(workspace.host), viewControls(settings.host), "the workspace capsule is the only difference");
      await act(async () => named(workspace.host, "Remove filter: Workspace Workspace A")!.click());
      await settle(() => renderedJobs(workspace.host).length === 5);
      assert.deepEqual(viewControls(workspace.host), viewControls(settings.host));
    } finally {
      await workspace.close();
      await settings.close();
    }
  });

  await runCases("Settings Jobs entry", tests, cleanup);
}

void main().catch((error) => { console.error(error); process.exitCode = 1; });
