// Run: npx tsx packages/chat-ui/src/blocks/LiveBlocks.test.tsx

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { AgentBlockInfo, AgentBlockState } from "./agentBlockModel";
import type { JobBlockInfo, JobBlockState } from "./jobBlockModel";
import type { ChecklistBlockState, ChecklistInfo } from "./checklistBlockModel";
import type { TasksBlockState } from "./tasksBlockModel";

const tests: Array<[string, () => Promise<void>]> = [];
function test(name: string, run: () => Promise<void>) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? testWindow : testWindow[key], configurable: true });
}
testWindow.matchMedia = (() => ({
  matches: false, media: "", onchange: null,
  addEventListener: () => {}, removeEventListener: () => {},
  addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
})) as unknown as typeof testWindow.matchMedia;

/** A live binding whose states the test sets; every set notifies like a host store change. */
function binding<T>(fallback: T) {
  const states = new Map<string, T>();
  const listeners = new Set<() => void>();
  const refreshed: string[] = [];
  return {
    refreshed,
    set(key: string, state: T) {
      states.set(key, state);
      for (const listener of listeners) listener();
    },
    source: {
      subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
      read: (key: string) => states.get(key) ?? fallback,
      refresh: (key: string) => { refreshed.push(key); },
    },
  };
}

const AGENT: AgentBlockInfo = {
  tabId: "e3ec9ee9-full", name: "Fermi", role: "Worker", provider: "Claude", model: "opus", state: "working",
  statusLine: null, task: { key: "BL-4", name: "Chat blocks", plan: "Blocks" }, peer: null, canStop: true,
};
const JOB: JobBlockInfo = {
  id: "cron_1", kind: "cron", name: "Morning digest", state: "enabled", stateLabel: "Enabled", schedule: "Every day at 09:00",
  nextRun: "in 3h", lastResult: { label: "Succeeded 1d ago", tone: "ok" }, canRunNow: true, canPause: true, canStop: true,
};
const CHECKLIST: ChecklistInfo = {
  id: "getting-started",
  title: "Getting started",
  items: [
    { id: "first-agent", label: "Start your first agent", hint: "Open a tab.", done: true, action: { kind: "link", label: "New agent" } },
    { id: "phone", label: "Connect your phone", hint: "Telegram.", done: false, action: { kind: "block", label: "Show how", source: "@ds-checklist nested-help" } },
    { id: "memory", label: "Save a memory", hint: "Remember.", done: null, action: { kind: "link", label: "Open Memory" } },
  ],
};

(async () => {
  const { RichMarkdown } = await import("../MarkdownBody");
  const { ChatBlockHostProvider } = await import("./ChatBlockHost");
  type Host = Parameters<typeof ChatBlockHostProvider>[0]["value"];

  async function withRender(text: string, value: Host, run: (element: HTMLDivElement) => Promise<void>) {
    const element = document.createElement("div");
    document.body.appendChild(element);
    const root = createRoot(element);
    try {
      await act(async () => root.render(<ChatBlockHostProvider value={value}><RichMarkdown text={text} /></ChatBlockHostProvider>));
      await run(element);
    } finally {
      await act(async () => root.unmount());
      element.remove();
    }
  }
  const action = (element: HTMLElement, id: string) => element.querySelector(`[data-block-action="${id}"]`) as HTMLButtonElement | null;
  const button = (element: HTMLElement, text: string) => [...element.querySelectorAll("button")].find((candidate) => candidate.textContent === text) as HTMLButtonElement | undefined;
  const status = (element: HTMLElement) => element.querySelector('[data-testid="chat-block-status"]')?.textContent;

  test("agent_card_follows_live_state_and_stop_needs_confirmation", async () => {
    const live = binding<AgentBlockState>({ kind: "loading" });
    const calls = { open: [] as string[], stop: [] as string[] };
    const agents = { binding: live.source, open: (ref: string) => { calls.open.push(ref); }, stop: async (ref: string) => { calls.stop.push(ref); } };
    await withRender("@ds-agent e3ec9ee9", { agents }, async (element) => {
      assert(element.querySelector('[data-chat-block="agent"][role="note"]'), "loading notice missing");
      assert(live.refreshed.includes("e3ec9ee9"), "card did not re-read on mount");
      await act(async () => live.set("e3ec9ee9", { kind: "ready", agent: AGENT }));
      assert(status(element) === "Working" && element.textContent?.includes("BL-4"), "working card lacks state or task");
      assert(calls.open.length === 0 && calls.stop.length === 0, "rendering acted");
      await act(async () => action(element, "stop")?.click());
      assert(calls.stop.length === 0, "stop ran without confirmation");
      await act(async () => button(element, "Stop turn")?.click());
      assert(calls.stop.length === 1 && calls.stop[0] === "e3ec9ee9", "confirmed stop did not run once");
      await act(async () => action(element, "open")?.click());
      assert(calls.open[0] === "e3ec9ee9", "open not routed");
      await act(async () => live.set("e3ec9ee9", { kind: "ready", agent: { ...AGENT, state: "idle", canStop: false, task: null } }));
      assert(status(element) === "Idle" && !action(element, "stop"), "idle agent still offers Stop");
      await act(async () => live.set("e3ec9ee9", { kind: "missing" }));
      assert(element.querySelector('[data-block-missing="true"]'), "closed tab not shown as missing");
    });
  });

  test("job_card_runs_now_on_click_and_pause_resume_confirm", async () => {
    const live = binding<JobBlockState>({ kind: "loading" });
    const calls: string[] = [];
    const jobs = {
      binding: live.source,
      open: (ref: string) => { calls.push(`open:${ref}`); },
      runNow: async (ref: string) => { calls.push(`run:${ref}`); },
      setPaused: async (ref: string, paused: boolean) => { calls.push(`${paused ? "pause" : "resume"}:${ref}`); },
      stop: async (ref: string) => { calls.push(`stop:${ref}`); },
    };
    live.set("cron_1", { kind: "ready", job: JOB });
    await withRender("@ds-job cron_1", { jobs }, async (element) => {
      assert(status(element) === "Enabled" && element.textContent?.includes("next in 3h"), "schedule or state missing");
      assert(element.querySelector('[data-testid="job-last-result"]')?.textContent?.includes("Succeeded"), "last result missing");
      assert(calls.length === 0, "rendering acted");
      await act(async () => { action(element, "run")?.click(); action(element, "run")?.click(); });
      assert(calls.filter((call) => call.startsWith("run")).length === 1, "run now ran more than once");
      await act(async () => action(element, "pause")?.click());
      assert(!calls.includes("pause:cron_1"), "pause ran without confirmation");
      await act(async () => button(element, "Pause")?.click());
      assert(calls.includes("pause:cron_1"), "confirmed pause did not run");
      await act(async () => live.set("cron_1", { kind: "ready", job: { ...JOB, state: "paused", stateLabel: "Paused" } }));
      assert(status(element) === "Paused" && action(element, "resume"), "paused job does not offer Resume");
      await act(async () => button(element, "Open in Jobs")?.click());
      assert(calls.includes("open:cron_1"), "open in jobs not routed");
      await act(async () => live.set("cron_1", { kind: "unavailable", message: "daemon offline" }));
      assert(element.textContent?.includes("daemon offline"), "unavailable not shown");
    });
  });

  test("checklist_ticks_from_state_and_items_act_only_on_click", async () => {
    const live = binding<ChecklistBlockState>({ kind: "missing" });
    const opened: string[] = [];
    live.set("getting-started", { kind: "ready", checklist: CHECKLIST });
    const checklists = { binding: live.source, open: (id: string, item: string) => { opened.push(`${id}/${item}`); } };
    await withRender("@ds-checklist getting-started", { checklists }, async (element) => {
      const row = (id: string) => element.querySelector(`[data-checklist-item="${id}"]`) as HTMLElement;
      assert(row("first-agent").dataset.done === "true" && row("phone").dataset.done === "false" && row("memory").dataset.done === "unknown", "item states wrong");
      assert(status(element) === "1 of 3 done", "progress label wrong");
      assert(!row("first-agent").querySelector("[data-checklist-action]"), "done item still offers its action");
      assert(opened.length === 0, "rendering acted");
      await act(async () => (row("memory").querySelector("[data-checklist-action]") as HTMLButtonElement).click());
      assert(opened[0] === "getting-started/memory", "link action not routed");
      await act(async () => (row("phone").querySelector("[data-checklist-action]") as HTMLButtonElement).click());
      assert(row("phone").querySelector('[data-testid="checklist-nested-block"] [data-chat-block="checklist"]'), "block action did not show the nested block");
      await act(async () => live.set("getting-started", { kind: "ready", checklist: { ...CHECKLIST, items: CHECKLIST.items.map((item) => ({ ...item, done: true })) } }));
      assert(status(element) === "All done", "checklist did not tick live");
    });
    await withRender("@ds-checklist nope", { checklists }, async (element) => {
      assert(element.querySelector('[data-block-missing="true"]'), "unknown checklist not missing");
    });
  });

  test("tasks_block_groups_live_rows_and_opens_on_click", async () => {
    const live = binding<TasksBlockState>({ kind: "loading" });
    const calls: string[] = [];
    const tasks = {
      binding: live.source,
      openPlan: (planId: string) => { calls.push(`plan:${planId}`); },
      openTask: (planId: string, taskId: string) => { calls.push(`task:${planId}/${taskId}`); },
    };
    await withRender("@ds-tasks pl_alpha in_progress", { tasks }, async (element) => {
      const key = live.refreshed[0];
      assert(key && JSON.parse(key)[0] === "pl_alpha", "binding key does not carry the plan");
      await act(async () => live.set(key, {
        kind: "ready", title: "Alpha plan", total: 2,
        groups: [
          { key: "in_progress", label: "In progress", collapsedByDefault: false, rows: [{ id: "task_a", key: "AL-1", name: "First", status: "in_progress", color: null, executor: "Fermi", model: "opus", progress: "2 strides", detached: false }] },
          { key: "done", label: "Done", collapsedByDefault: true, rows: [{ id: "task_b", key: "AL-2", name: "Second", status: "done", color: null, executor: null, model: null, progress: null, detached: false }] },
        ],
      }));
      assert(element.querySelector('[data-chat-block="tasks"] h4')?.textContent === "Alpha plan", "plan title missing");
      assert(element.querySelector('[data-task-row="task_a"]') && !element.querySelector('[data-task-row="task_b"]'), "done group should start collapsed");
      assert(calls.length === 0, "rendering acted");
      await act(async () => (element.querySelector('[data-task-row="task_a"]') as HTMLButtonElement).click());
      await act(async () => button(element, "Open plan")?.click());
      assert(calls.join(",") === "task:pl_alpha/task_a,plan:pl_alpha", `unexpected calls ${calls.join(",")}`);
    });
  });

  test("blocks_without_their_service_render_a_note_and_no_buttons", async () => {
    await withRender("@ds-agent e3ec9ee9\n@ds-job cron_1\n@ds-checklist getting-started\n@ds-tasks pl_alpha", {}, async (element) => {
      assert(element.querySelectorAll('[data-testid="chat-block"][role="note"]').length === 4, "expected four notes");
      assert(!element.querySelector("button"), "hostless view must not offer actions");
    });
  });

  let failed = 0;
  for (const [name, run] of tests) {
    try {
      await run();
      console.log(`  ✓ ${name}`);
    } catch (error) {
      failed++;
      console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
    }
  }
  console.log(`chat-ui/blocks/LiveBlocks tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
