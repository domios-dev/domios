import { configureOrchestrationTransport } from "../../lib/transport/orchestrationRuntime";
import { orchestrationTestClient } from "../../lib/transport/orchestration.testdata";
// Run: node scripts/run-ts-unit-tests.mjs src/components/Terminal/ComposerContextHeader.test.tsx
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { CronJob, ProviderDescriptor } from "../../lib/types";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import type { AutomationGraphAdapter } from "../Automation/types";
import type { DelayedMessage } from "./delayedSend";
import type { ExtendedState } from "../../store/multiplexer/tree";

function requireControl<T extends Element>(selector: string): T {
  const current = document.querySelector<T>(selector);
  assert.ok(current, `Missing control: ${selector}`);
  return current;
}

function bubble(host: HTMLElement, title: string): HTMLButtonElement | null {
  return Array.from(host.querySelectorAll<HTMLButtonElement>(".ct-ctx-bubble__open"))
    .find((button) => button.querySelector(".ct-ctx-bubble__title")?.textContent === title) ?? null;
}

function assertScrollablePanel(label: string) {
  const panel = requireControl<HTMLElement>(`[role="group"][aria-label="${label}"]`);
  const style = window.getComputedStyle(panel);
  assert.equal(style.maxHeight, "200px");
  assert.equal(style.overflowY, "auto");
  assert.equal(style.overscrollBehaviorY, "contain");
  assert.equal(style.touchAction, "pan-y");
  assert.equal(panel.tabIndex, 0);
  panel.focus();
  assert.equal(document.activeElement, panel, "keyboard users can focus the scroll region");
  assert.equal(panel.contains(document.querySelector('[aria-label="Linked to this tab"]')), false, "the header stays outside the scrolling body");
}

async function main() {
  const win = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "localStorage", "HTMLElement", "HTMLButtonElement", "SVGElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const stylesheet = document.createElement("style");
  stylesheet.textContent = readFileSync(new URL("./composerContext.css", import.meta.url), "utf8");
  document.head.append(stylesheet);

  const [{ __setInvokeForTest, __setListenForTest }, { ComposerContextHeader }, { OPEN_JOB_EVENT }, { MuxStateStoreContext }, { createMuxStateStore }] = await Promise.all([
    import("../../lib/bridge"),
    import("./ComposerContextHeader"),
    import("../Jobs/openJobEvent"),
    import("../../store/multiplexer/context"),
    import("../../store/multiplexer/selectorStore"),
  ]);
  const state = {
    workspaces: [], tabs: {
      "tab-a": { agent_type: "codex" }, "tab-b": { agent_type: "codex" }, "claude-tab": { agent_type: "claude" },
    }, teams: [], active_workspace_id: null, focused_tab_id: null,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 0, pendingCloses: [],
  } as unknown as ExtendedState;
  const store = createMuxStateStore(() => state);
  setAgentDescriptors([
    { id: "codex", agent_runtime: { goals: { actions: ["read"] } } },
    { id: "claude" },
  ] as unknown as ProviderDescriptor[]);
  const goalReads: unknown[] = [];
  let failGoalRead = false;

  const job: CronJob = {
    id: "cron-rss", name: "rust-rss-new-videos", description: null, workspace_id: "workspace-a", team_id: null, schedule: "*/5 * * * *", enabled: true,
    agent: "agent-fixture", model: null, reasoning_effort: null, task: "task", cwd: null, role: null, creator_tab_id: "tab-a", report_target_tab_id: null,
    oneshot: false, max_history: 10, created_at: 1, next_run: null, jitter_max_ms: null,
    last_run: { run_id: "run-1", started_at: 1, finished_at: null, status: "Running", tab_id: "run-tab" },
  };
  const jobs = [job, ...[2, 3, 4, 5].map((index) => ({ ...job, id: `cron-${index}`, name: `run-${index}` }))];
  configureOrchestrationTransport(orchestrationTestClient(async <T,>(_method: string, path: string) => {
    assert.equal(path, "/api/crons");
    return jobs as T;
  }));
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "cron_list") return jobs as T;
    if (command === "agent_runtime_goal") {
      goalReads.push(args);
      if (failGoalRead) throw new Error("Fixture read failure");
      return { supported: true, intent: { goal: "Read objective" }, actions: [] } as T;
    }
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
    ...delayed, id: "system:heartbeat:pl:tab-a", origin: "system", text: '<codeterm-orchestrator-heartbeat plan="pl">\ncharter-frag\n</codeterm-orchestrator-heartbeat>',
    ticks: [{ atMs: 0, outcome: "skipped", spinalVerdict: "skipped", spinalReason: "not_due" }],
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
      heartbeats={messages.filter(message => message.origin === "system")}
      onEditDelayed={(message) => edited.push(message.id)}
      onDeleteDelayed={() => undefined}
      onScheduleDelayed={() => undefined}
      draftActive={draftActive}
      adapter={adapter}
    /></MuxStateStoreContext.Provider>,
  );
  await act(async () => { render("tab-a", [stashed, delayed, system]); });
  assert.equal(host.querySelector('[data-kind="delayed"]')?.getAttribute("aria-label"), "3 delayed messages, system reminder", "the heartbeat counts as a delayed message");
  assert.equal(host.querySelectorAll('button[title="Heartbeat"]').length, 0);
  const delayedChip = requireControl<HTMLButtonElement>('button[title="Delayed messages"]');
  assert.ok(delayedChip.querySelector("svg"), "delayed chip retains its icon");
  await act(async () => { delayedChip.click(); });
  assertScrollablePanel("Delayed messages");
  const heartbeatRow = requireControl<HTMLButtonElement>('[aria-label^="System reminder:"]');
  assert.match(heartbeatRow.textContent ?? "", /^System/, "the capsule is labelled System");
  for (const text of [heartbeatRow.textContent ?? "", heartbeatRow.getAttribute("aria-label") ?? ""]) assert.ok(!text.includes("codeterm-"), "no internal marker in the capsule");
  assert.equal(heartbeatRow.getAttribute("aria-expanded"), "false");
  assert.equal(heartbeatRow.parentElement?.querySelectorAll("button").length, 1, "heartbeat cannot be cancelled or disabled");
  assert.equal(document.querySelectorAll('[aria-label="Cancel delayed message"]').length, 2);
  await act(async () => { heartbeatRow.click(); });
  const detail = document.querySelector('[role="dialog"][aria-labelledby="system-heartbeat-title"]');
  assert.equal(heartbeatRow.getAttribute("aria-expanded"), "true");
  assert.match(document.getElementById("system-heartbeat-title")?.textContent ?? "", /^System/);
  assert.equal(detail?.querySelector('[role="region"][aria-label="Reminder text"]')?.textContent?.includes("charter-frag"), true, "details show the readable reminder text");
  assert.equal(detail?.textContent?.includes("codeterm-"), false, "details never show the raw marker");
  assert.match(detail?.querySelector('[data-testid="heartbeat-reason"]')?.textContent ?? "", /task_a/);
  assert.equal(detail?.querySelector('[data-testid="heartbeat-tick"]'), null, "no execution log rows");
  await act(async () => { document.querySelector<HTMLButtonElement>('[aria-label="Close"]')!.click(); });
  assert.equal(document.querySelector('[aria-labelledby="system-heartbeat-title"]'), null, "Close dismisses the details");
  await act(async () => { render("tab-a", [system, { ...delayed, id: "due", scheduledAtMs: 0 }, { ...delayed, id: "sending", state: "sending" }]); });
  assert.equal(delayedChip.getAttribute("aria-label"), "1 delayed message, system reminder");
  assert.ok(delayedChip.querySelector("svg"), "heartbeat-only delayed chip retains its icon");
  assert.equal(requireControl('[aria-label^="System reminder:"]'), heartbeatRow, "queue transitions preserve the heartbeat row");
  assert.equal(document.querySelectorAll('[aria-label="Cancel delayed message"]').length, 0);
  await act(async () => { render("tab-a", [stashed, delayed, system]); });

  const crons = requireControl<HTMLButtonElement>('.ct-ctx-kind[data-kind="crons"]');
  assert.equal(crons.querySelector(".ct-ctx-kind__count")?.textContent, "5", "the cron button counts every cron bound to the tab");
  const blueprints = requireControl<HTMLButtonElement>('.ct-ctx-kind[data-kind="blueprints"]');
  assert.equal(blueprints.querySelector(".ct-ctx-kind__count"), null, "a single blueprint shows no count");
  assert.match(host.querySelector(".ct-ctx__title")?.textContent ?? "", /Blueprint:Echo pipeline/, "with no plan, the owned blueprint heads the capsule");

  await act(async () => { blueprints.click(); });
  assertScrollablePanel("Blueprints");
  assert.equal(blueprints.getAttribute("aria-pressed"), "true", "the pressed kind reports its state");
  assert.equal(host.querySelector(".ct-ctx__reveal")?.hasAttribute("data-open"), true, "pressing a kind unfolds its strip");
  await act(async () => { bubble(host, "Echo pipeline")!.click(); });

  await act(async () => { crons.click(); });
  assertScrollablePanel("Crons");
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
  assert.ok(host.querySelector('[role="group"][aria-label="Delayed messages"] [aria-label^="System reminder:"]'), "heartbeat appears inside the delayed list");
  assert.ok(host.querySelector('[aria-label^="Edit delayed message"]')?.parentElement?.querySelector(".ct-ctx-bubble__dismiss"), "a user delayed message stays cancellable");
  await act(async () => { host.querySelector<HTMLButtonElement>('[aria-label^="Edit delayed message"]')!.click(); });
  assert.deepEqual(edited, ["later-1"], "a delayed bubble opens its editor");

  await act(async () => { crons.click(); });
  await act(async () => { host.querySelector<HTMLButtonElement>('.ct-ctx-kind[data-kind="crons"]')!.click(); });
  assert.equal(host.querySelector(".ct-ctx__reveal"), null, "pressing the open kind again removes the strip");

  await act(async () => { host.querySelector<HTMLButtonElement>('.ct-ctx-kind[data-kind="crons"]')!.click(); });
  assert.equal(host.querySelector(".ct-ctx__reveal")?.hasAttribute("data-open"), true);
  await act(async () => { render("tab-a", [stashed, delayed], true); });
  assert.equal(host.querySelector(".ct-ctx__reveal"), null, "starting a draft removes the open strip");

  await act(async () => { render("tab-b", []); });
  assert.equal(host.querySelector('[data-testid="composer-context-header"]'), null, "a tab with nothing linked shows no header");

  for (const width of [1280, 390]) {
    await act(async () => { win.happyDOM.setWindowSize({ width, height: 900 }); });
    for (const intent of [{ title: "Claude title" }, { title: "Claude title", goal: "Foreign goal" }]) {
      await act(async () => { root.render(null); });
      await act(async () => { root.render(
        <MuxStateStoreContext.Provider value={store}><ComposerContextHeader
          tabId="claude-tab" workspaceId="workspace-a" delayed={[]} adapter={adapter} sessionIntent={intent}
          onEditDelayed={() => undefined} onDeleteDelayed={() => undefined} onScheduleDelayed={() => undefined}
        /></MuxStateStoreContext.Provider>,
      ); });
      assert.equal(host.childElementCount, 0);
      assert.equal(goalReads.length, 0);
    }
    for (const theme of ["light", "dark"]) {
      document.documentElement.dataset.theme = theme;
      await act(async () => { root.render(null); });
      await act(async () => { root.render(
        <MuxStateStoreContext.Provider value={store}><ComposerContextHeader
          tabId="tab-a" workspaceId="workspace-a" delayed={[delayed]} adapter={adapter}
          sessionIntent={{ goal: "Fixture goal" }}
          pluginError={JSON.stringify({ code: "chat_backend_plugin_removed", plugin_id: "fixture" })}
          onEditDelayed={() => undefined} onDeleteDelayed={() => undefined} onScheduleDelayed={() => undefined}
        /></MuxStateStoreContext.Provider>,
      ); });
      const group = host.querySelector<HTMLElement>('[role="group"][aria-label="Linked to this tab"]')!;
      assert.ok(group);
      for (const label of ["Session goal", "Plugin removed", "1 blueprint", "5 crons", "1 delayed message"]) {
        const chip = host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
        assert.ok(chip, label);
        assert.equal(chip.closest('[role="group"]'), group);
        assert.ok(chip.querySelector("svg"), `${label} retains its icon at ${width}px`);
        if (label !== "Plugin removed") {
          assert.equal(window.getComputedStyle(chip).flexShrink, "0");
          assert.equal(window.getComputedStyle(chip.querySelector("svg")!).flexShrink, "0");
        }
      }
      const fold = host.querySelector<HTMLButtonElement>('button[aria-controls]');
      if (width === 390) {
        assert.ok(fold);
        assert.equal(fold.getAttribute("aria-controls"), group.id);
        assert.equal(group.hidden, true);
        assert.equal(fold.getAttribute("aria-expanded"), "false");
        await act(async () => { fold.click(); });
        assert.equal(group.hidden, false);
        assert.equal(fold.getAttribute("aria-expanded"), "true");
        await act(async () => { fold.click(); });
        assert.equal(group.hidden, true);
        await act(async () => { root.render(null); });
        await act(async () => { root.render(
          <MuxStateStoreContext.Provider value={store}><ComposerContextHeader
            tabId="tab-b" workspaceId="workspace-a" delayed={[]} adapter={adapter}
            sessionIntent={{ goal: "Standalone goal" }}
            onEditDelayed={() => undefined} onDeleteDelayed={() => undefined} onScheduleDelayed={() => undefined}
          /></MuxStateStoreContext.Provider>,
        ); });
        const goalOnlyGroup = host.querySelector<HTMLElement>('[role="group"][aria-label="Linked to this tab"]')!;
        const goalOnlyFold = host.querySelector<HTMLButtonElement>('button[aria-controls]')!;
        assert.ok(goalOnlyFold);
        assert.equal(goalOnlyGroup.hidden, true);
        await act(async () => { goalOnlyFold.click(); });
        assert.equal(goalOnlyGroup.hidden, false);
        assert.ok(goalOnlyGroup.querySelector('button[aria-label="Session goal"]'));
      } else {
        assert.equal(fold, null);
        assert.equal(group.hidden, false);
      }
      assert.equal(host.querySelector('[role="dialog"][aria-label="Session goal"]'), null);
      const goalChip = host.querySelector<HTMLButtonElement>('button[aria-label="Session goal"]')!;
      const delayedChip = host.querySelector<HTMLButtonElement>('button[aria-label="1 delayed message"]');
      if (delayedChip) assert.equal(goalChip.parentElement, delayedChip.parentElement);
      await act(async () => { goalChip.click(); });
      assert.ok(host.querySelector('[role="dialog"][aria-label="Session goal"]'));
      if (delayedChip) {
        await act(async () => { delayedChip.click(); });
        assert.equal(host.querySelector('[role="dialog"][aria-label="Session goal"]'), null);
        assert.ok(host.querySelector('[role="group"][aria-label="Delayed messages"]'));
        await act(async () => { goalChip.click(); });
      }
      await act(async () => { document.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
      assert.equal(host.querySelector('[role="dialog"][aria-label="Session goal"]'), null);
    }
    goalReads.length = 0;
  }

  failGoalRead = true;
  await act(async () => { root.render(null); });
  await act(async () => { root.render(
    <MuxStateStoreContext.Provider value={store}><ComposerContextHeader
      tabId="tab-b" workspaceId="workspace-a" delayed={[]} adapter={adapter} sessionIntent={{ goal: "Failed read" }}
      onEditDelayed={() => undefined} onDeleteDelayed={() => undefined} onScheduleDelayed={() => undefined}
    /></MuxStateStoreContext.Provider>,
  ); });
  await act(async () => { host.querySelector<HTMLButtonElement>('button[aria-label="Session goal"]')!.click(); });
  assert.equal(goalReads.length, 1);
  assert.equal(host.childElementCount, 0, "a failed read removes the button, strip, and any error UI");

  await act(async () => { root.unmount(); });
  console.log("Composer context header wiring passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
