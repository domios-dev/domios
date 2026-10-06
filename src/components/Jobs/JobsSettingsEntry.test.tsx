// Run: node scripts/run-ts-unit-tests.mjs src/components/Jobs/JobsSettingsEntry.test.tsx (SettingsPanel imports image assets)
import assert from "node:assert/strict";
import { act } from "react";
import type { AppConfig } from "../../lib/types";
import { entry, foreignCron, installBridge, installDom, mount, named, removeCapsules, renderedJobs, runCases, scenarioB, settle, teams, workspaces } from "./jobsEntry.testdata";

async function main() {
  installDom();
  const cleanup = await installBridge();
  const { SettingsPanel } = await import("../Settings/SettingsPanel");

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
      assert.equal(named(view.host, "Close jobs"), null, "Settings owns closing");
    } finally { await view.close(); }
  });

  await runCases("Settings Jobs entry", tests, cleanup);
}

void main().catch((error) => { console.error(error); process.exitCode = 1; });
