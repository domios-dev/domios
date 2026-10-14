// Run: node scripts/run-ts-unit-tests.mjs src/components/Terminal/ComposerContextHeader.test.tsx
import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { CronJob } from "../../lib/types";
import type { AutomationGraphAdapter } from "../Automation/types";
import type { DelayedMessage } from "./delayedSend";
import type { ExtendedState } from "../../store/multiplexer/tree";

async function waitForSelector<T extends Element>(selector: string): Promise<T> {
  const current = document.querySelector<T>(selector);
  if (current) return current;
  return new Promise((resolve) => {
    const observer = new window.MutationObserver(() => {
      const element = document.querySelector<T>(selector);
      if (!element) return;
      observer.disconnect();
      resolve(element);
    });
    observer.observe(document.body, { childList: true, subtree: true });
  });
}

function bubble(host: HTMLElement, title: string): HTMLButtonElement | null {
  return Array.from(host.querySelectorAll<HTMLButtonElement>(".ct-ctx-bubble__open"))
    .find((button) => button.querySelector(".ct-ctx-bubble__title")?.textContent === title) ?? null;
}

async function main() {
  const win = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "localStorage", "HTMLElement", "HTMLButtonElement", "SVGElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

  const [{ __setInvokeForTest, __setListenForTest }, { ComposerContextHeader }, { OPEN_JOB_EVENT }, { MuxStateStoreContext }, { createMuxStateStore }] = await Promise.all([
    import("../../lib/bridge"),
    import("./ComposerContextHeader"),
    import("../Jobs/openJobEvent"),
    import("../../store/multiplexer/context"),
    import("../../store/multiplexer/selectorStore"),
  ]);
  const state = {
    workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 0, pendingCloses: [],
  } as unknown as ExtendedState;
  const store = createMuxStateStore(() => state);

  const job: CronJob = {
    id: "cron-rss", name: "rust-rss-new-videos", description: null, workspace_id: "workspace-a", team_id: null, schedule: "*/5 * * * *", enabled: true,
    agent: "agent-fixture", model: null, reasoning_effort: null, task: "task", cwd: null, role: null, creator_tab_id: "tab-a", report_target_tab_id: null,
    oneshot: false, max_history: 10, created_at: 1, next_run: null, jitter_max_ms: null,
    last_run: { run_id: "run-1", started_at: 1, finished_at: null, status: "Running", tab_id: "run-tab" },
  };
  const jobs = [job, ...[2, 3, 4, 5].map((index) => ({ ...job, id: `cron-${index}`, name: `run-${index}` }))];
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "cron_list") return jobs as T;
    throw new Error(`unexpected invoke: ${command}`);
  });
  __setListenForTest(async () => () => undefined);
  const adapter: AutomationGraphAdapter = {
    read: async (scope) => ({
      workspace_id: scope.workspace_id, team_id: null, all_teams: true, revision: "r", scenarios: [], nodes: [], edges: [],
      blueprints: [{
        id: "blueprint-echo", name: "Echo pipeline", owner: { kind: "tab", tab_id: "tab-a", team_id: null }, state: "running", run_id: "run-a", current_step: 0, last_event_at_ms: 100,
        trigger: { type: "event", summary: "Agent message" },
        steps: [{ index: 0, name: "Echo", kind: "command", title: "Echo", state: "running", outcome: null, started_at_ms: 50, ended_at_ms: null, result_summary: null, reason: null }],
        result: { target: { kind: "owner_tab", id: "tab-a" }, state: "waiting", reason: null, body_ref: null },
      }],
    }),
    subscribe: async () => () => undefined,
    stop: async () => undefined,
    delete: async () => undefined,
  };
  const delayed: DelayedMessage = {
    id: "later-1", tabId: "tab-a", text: "Re-run the flaky sidebar suite and post a summary", scheduledAtMs: Date.now() + 12 * 60_000,
    createdAtMs: 1, state: "queued", sessionChanged: false,
  };

  const stashed: DelayedMessage = { ...delayed, id: "stash-1", text: "Keep this for later", state: "stashed", scheduledAtMs: 0 };
  const system: DelayedMessage = {
    ...delayed, id: "system:heartbeat:pl:tab-a", origin: "system", text: "Plan reminder",
    reason: "sent because tasks task_a are still in progress in plan pl", recurrence: { everyMs: 60_000 },
  };

  const opened: unknown[] = [];
  const edited: string[] = [];
  window.addEventListener(OPEN_JOB_EVENT, (event) => opened.push((event as CustomEvent).detail));
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const render = (tabId: string, messages: DelayedMessage[], draftActive = false) => root.render(
    <MuxStateStoreContext.Provider value={store}><ComposerContextHeader
      tabId={tabId}
      workspaceId="workspace-a"
      delayed={messages}
      onEditDelayed={(message) => edited.push(message.id)}
      onDeleteDelayed={() => undefined}
      onScheduleDelayed={() => undefined}
      draftActive={draftActive}
      adapter={adapter}
    /></MuxStateStoreContext.Provider>,
  );
  await act(async () => { render("tab-a", [stashed, delayed, system]); });

  const crons = await waitForSelector<HTMLButtonElement>('.ct-ctx-kind[data-kind="crons"]');
  assert.equal(crons.querySelector(".ct-ctx-kind__count")?.textContent, "5", "the cron button counts every cron bound to the tab");
  const blueprints = await waitForSelector<HTMLButtonElement>('.ct-ctx-kind[data-kind="blueprints"]');
  assert.equal(blueprints.querySelector(".ct-ctx-kind__count"), null, "a single blueprint shows no count");
  assert.match(host.querySelector(".ct-ctx__title")?.textContent ?? "", /Blueprint:Echo pipeline/, "with no plan, the owned blueprint heads the capsule");

  await act(async () => { blueprints.click(); });
  assert.equal(blueprints.getAttribute("aria-pressed"), "true", "the pressed kind reports its state");
  assert.equal(host.querySelector(".ct-ctx__reveal")?.hasAttribute("data-open"), true, "pressing a kind unfolds its strip");
  await act(async () => { bubble(host, "Echo pipeline")!.click(); });

  await act(async () => { crons.click(); });
  assert.equal(host.querySelector(".ct-ctx__strip")?.getAttribute("data-kind"), "crons", "switching kinds swaps the strip");
  assert.equal(host.querySelectorAll(".ct-ctx__strip .ct-ctx-bubble").length, 5, "every bound cron gets a bubble");
  await act(async () => { bubble(host, "rust-rss-new-videos")!.click(); });
  assert.deepEqual(opened, [
    { workspaceId: "workspace-a", target: { teamId: null, blueprintId: "blueprint-echo" }, returnTo: { workspaceId: "workspace-a", tabId: "tab-a" } },
    { workspaceId: "workspace-a", target: { teamId: null, cronId: "cron-rss" }, returnTo: { workspaceId: "workspace-a", tabId: "tab-a" } },
  ], "bubbles open the exact job in Jobs");

  await act(async () => { host.querySelector<HTMLButtonElement>('.ct-ctx-kind[data-kind="delayed"]')!.click(); });
  const delayedBubbles = Array.from(host.querySelectorAll(".ct-ctx__strip .ct-ctx-bubble:not(.ct-ctx-bubble--add)"));
  const lastDelayed = delayedBubbles[delayedBubbles.length - 1];
  assert.equal(lastDelayed?.getAttribute("data-tone"), "stashed", "stashed messages sort after scheduled ones");
  assert.ok(lastDelayed?.querySelector(".ct-ctx-bubble__stash"), "a stashed message shows the stash mark instead of a time");
  assert.equal(lastDelayed?.querySelector(".ct-ctx-bubble__meta"), null, "a stashed message shows no send time");
  const systemOpen = host.querySelector('[aria-label^="System heartbeat"]');
  assert.equal(systemOpen?.parentElement?.querySelector(".ct-ctx-bubble__dismiss"), null, "a system heartbeat has no cancel");
  assert.ok(host.querySelector('[aria-label^="Edit delayed message"]')?.parentElement?.querySelector(".ct-ctx-bubble__dismiss"), "a user delayed message stays cancellable");
  await act(async () => { host.querySelector<HTMLButtonElement>('[aria-label^="Edit delayed message"]')!.click(); });
  assert.deepEqual(edited, ["later-1"], "a delayed bubble opens its editor");

  await act(async () => { crons.click(); });
  await act(async () => { host.querySelector<HTMLButtonElement>('.ct-ctx-kind[data-kind="crons"]')!.click(); });
  assert.equal(host.querySelector(".ct-ctx__reveal")?.hasAttribute("data-open"), false, "pressing the open kind again folds the strip");

  await act(async () => { host.querySelector<HTMLButtonElement>('.ct-ctx-kind[data-kind="crons"]')!.click(); });
  assert.equal(host.querySelector(".ct-ctx__reveal")?.hasAttribute("data-open"), true);
  await act(async () => { render("tab-a", [stashed, delayed], true); });
  assert.equal(host.querySelector(".ct-ctx__reveal")?.hasAttribute("data-open"), false, "starting a draft folds the open strip away");

  await act(async () => { render("tab-b", []); });
  assert.equal(host.querySelector('[data-testid="composer-context-header"]'), null, "a tab with nothing linked shows no header");

  await act(async () => { root.unmount(); });
  console.log("Composer context header wiring passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
