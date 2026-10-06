// Run: node scripts/run-ts-unit-tests.mjs src/components/Terminal/ComposerJobChips.test.tsx
import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { CronJob } from "../../lib/types";
import type { AutomationGraphAdapter } from "../Automation/types";

async function main() {
  const win = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "localStorage", "HTMLElement", "HTMLButtonElement", "SVGElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

  const [{ __setInvokeForTest, __setListenForTest }, { ComposerJobChips }, { OPEN_JOB_EVENT }] = await Promise.all([
    import("../../lib/bridge"),
    import("./ComposerJobChips"),
    import("../Jobs/openJobEvent"),
  ]);

  const job: CronJob = {
    id: "cron-rss", name: "rust-rss-new-videos", description: null, workspace_id: "workspace-a", team_id: null, schedule: "*/5 * * * *", enabled: true,
    agent: "agent-fixture", model: null, reasoning_effort: null, task: "task", cwd: null, role: null, creator_tab_id: "tab-a", report_target_tab_id: null,
    oneshot: false, max_history: 10, created_at: 1, next_run: null, jitter_max_ms: null,
    last_run: { run_id: "run-1", started_at: 1, finished_at: null, status: "Running", tab_id: "run-tab" },
  };
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "cron_list") return [job] as T;
    throw new Error(`unexpected invoke: ${command}`);
  });
  __setListenForTest(async () => () => undefined);
  const adapter: AutomationGraphAdapter = {
    read: async (scope) => ({ workspace_id: scope.workspace_id, team_id: null, all_teams: true, revision: "r", scenarios: [], nodes: [], edges: [] }),
    subscribe: async () => () => undefined,
    stop: async () => undefined,
    delete: async () => undefined,
  };

  const opened: unknown[] = [];
  window.addEventListener(OPEN_JOB_EVENT, (event) => opened.push((event as CustomEvent).detail));
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(<ComposerJobChips tabId="tab-a" workspaceId="workspace-a" adapter={adapter} />); });

  const chip = host.querySelector<HTMLButtonElement>('button[aria-label="Job rust-rss-new-videos, running"]');
  assert.ok(chip, "a bound running cron renders a chip with its accessible name");
  await act(async () => { chip.click(); });
  assert.deepEqual(opened, [{ workspaceId: "workspace-a", target: { teamId: null, cronId: "cron-rss" } }], "clicking opens the Jobs view on that job once");

  await act(async () => { root.render(<ComposerJobChips tabId="tab-b" workspaceId="workspace-a" adapter={adapter} />); });
  assert.equal(host.querySelector('[aria-label^="Job "]'), null, "an unbound tab shows no job chip");

  await act(async () => { root.unmount(); });
  console.log("Composer job chip wiring passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
