// Run: npx tsx src/components/Canvas/PlanViewShell.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { ApiConfig } from "../../lib/types";
import { __setListenForTest } from "../../lib/bridge";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import { PlanViewShell } from "./PlanViewShell";
import { PlansProvider } from "./PlansProvider";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const PLAN_VIEW_MODE_STORAGE_KEY = "codeterm.planViewMode";

const STABLE_TABS = {};
const SHELL_PROPS = {
  mode: "workspace" as const,
  plans: [FIXTURE_PLAN],
  tabs: STABLE_TABS,
  viewportPadding: 10000,
};

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1:7686/plan-view-shell" });
  Object.assign(window, { __CODETERM_DAEMON__: { origin: "http://127.0.0.1:7686", token: "test-token" } });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    value: class {
      observe() {}
      disconnect() {}
    },
  });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  (globalThis as unknown as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 0) as unknown as number;
  __setListenForTest(async () => () => {});
}

async function render(ui: ReactElement) {
  setupDom();
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return {
    host,
    root,
    rerender: async (next: ReactElement) => {
      await act(async () => {
        root.render(next);
      });
    },
    unmount: async () => {
      await act(async () => {
        root.unmount();
      });
      host.remove();
    },
  };
}

function clickTab(host: HTMLElement, label: string) {
  const tabs = host.querySelectorAll('[data-testid="plan-view-mode-switch"] [role="tab"]');
  for (const tab of tabs) {
    if (tab.textContent?.trim() === label) {
      (tab as HTMLButtonElement).click();
      return;
    }
  }
  throw new Error(`tab not found: ${label}`);
}

function panelFor(host: HTMLElement, testId: string): HTMLElement | null {
  return host.querySelector(`[data-testid="${testId}"]`)?.parentElement ?? null;
}

test("PlanViewShell defaults to board mode", async () => {
  setupDom();
  window.localStorage.removeItem(PLAN_VIEW_MODE_STORAGE_KEY);
  const { host, unmount } = await render(<PlanViewShell {...SHELL_PROPS} />);
  const shell = host.querySelector('[data-testid="plan-view-shell"]');
  assert(shell?.getAttribute("data-plan-view-mode") === "board", "default mode attr");
  const modeLabels = Array.from(host.querySelectorAll('[data-testid="plan-view-mode-switch"] [role="tab"]'))
    .map((tab) => tab.textContent?.trim());
  assert(modeLabels.join(",") === "Board,Spec,Plan,Tasks", `switcher entries=${modeLabels.join(",")}`);
  assert(!modeLabels.includes("List") && !modeLabels.includes("Canvas"), "parked views absent from switcher");
  const missionControl = panelFor(host, "mission-control-plan-surface");
  assert(missionControl && !missionControl.hasAttribute("hidden"), "mission control visible");
  const list = panelFor(host, "plan-list-view");
  assert(list?.hasAttribute("hidden"), "list hidden");
  await unmount();
});

test("PlanViewShell restores persisted mode from localStorage", async () => {
  setupDom();
  window.localStorage.setItem(PLAN_VIEW_MODE_STORAGE_KEY, "list");
  const hostEl = document.createElement("div");
  document.body.appendChild(hostEl);
  const root: Root = createRoot(hostEl);
  await act(async () => {
    root.render(<PlanViewShell {...SHELL_PROPS} />);
  });
  const shell = hostEl.querySelector('[data-testid="plan-view-shell"]');
  assert(shell?.getAttribute("data-plan-view-mode") === "board", "parked mode resolves to board");
  const missionControl = panelFor(hostEl, "mission-control-plan-surface");
  assert(missionControl && !missionControl.hasAttribute("hidden"), "mission control visible");
  await act(async () => {
    root.unmount();
  });
  hostEl.remove();
});

test("PlanViewShell persists Spec mode changes to localStorage", async () => {
  setupDom();
  window.localStorage.removeItem(PLAN_VIEW_MODE_STORAGE_KEY);
  const { host, unmount } = await render(<PlanViewShell {...SHELL_PROPS} />);
  await act(async () => {
    clickTab(host, "Spec");
  });
  assert(window.localStorage.getItem(PLAN_VIEW_MODE_STORAGE_KEY) === "spec", "spec persisted");
  await unmount();
});

test("PlanViewShell switches between Spec, Plan, and Board without losing shell target", async () => {
  setupDom();
  window.localStorage.removeItem(PLAN_VIEW_MODE_STORAGE_KEY);
  const { host, unmount } = await render(<PlanViewShell {...SHELL_PROPS} />);
  await act(async () => {
    clickTab(host, "Plan");
  });
  const shell = host.querySelector('[data-testid="plan-view-shell"]');
  assert(shell?.getAttribute("data-plan-view-mode") === "plan", "plan mode attr");
  const plan = panelFor(host, "plan-plan-surface");
  assert(plan && !plan.hasAttribute("hidden"), "plan visible");
  const board = panelFor(host, "mission-control-plan-surface");
  assert(board?.hasAttribute("hidden"), "board hidden but mounted");

  await act(async () => {
    clickTab(host, "Spec");
  });
  assert(shell?.getAttribute("data-plan-view-mode") === "spec", "spec mode attr");
  const spec = panelFor(host, "plan-spec-surface");
  assert(spec && !spec.hasAttribute("hidden"), "spec visible");
  const tasks = panelFor(host, "plan-board-view");
  assert(tasks?.hasAttribute("hidden"), "tasks hidden but mounted");

  await act(async () => {
    clickTab(host, "Board");
  });
  assert(shell?.getAttribute("data-plan-view-mode") === "board", "board mode attr");
  await act(async () => {
    clickTab(host, "Tasks");
  });
  assert(shell?.getAttribute("data-plan-view-mode") === "tasks", "tasks mode attr");
  assert(tasks && !tasks.hasAttribute("hidden"), "tasks visible");
  await act(async () => {
    clickTab(host, "Board");
  });
  assert(shell?.getAttribute("data-plan-view-mode") === "board", "board mode attr");
  assert(board && !board.hasAttribute("hidden"), "board visible");
  await unmount();
});

test("Tasks mode requests one compact page without subscribing to full plans", async () => {
  setupDom();
  window.localStorage.setItem(PLAN_VIEW_MODE_STORAGE_KEY, "tasks");
  const api: ApiConfig = { enabled: true, host: "127.0.0.1", port: 7686, token: "test-token", cors: true };
  const entry = {
    plan_id: FIXTURE_PLAN.id,
    file_path: "/tmp/plan.json",
    canonical_project_path: FIXTURE_PLAN.project_path,
    owner: { ...FIXTURE_PLAN.owner, tab_id: "orch-1" },
    team_id: null,
    title: FIXTURE_PLAN.title,
    status: "active" as const,
    updated_at: FIXTURE_PLAN.updated_at,
    assigned_tab_ids: [],
    task_count: 1,
    open_task_count: 1,
    section_count: 0,
    element_count: 0,
    loop_count: 0,
    has_goal: false,
  };
  let fullCalls = 0;
  let pageCalls = 0;
  const requestPaths: string[] = [];
  globalThis.fetch = async (input) => {
    const url = new URL(String(input));
    requestPaths.push(`${url.pathname}${url.search}`);
    if (url.searchParams.get("full") === "1") {
      fullCalls++;
      return Response.json({ plans: [FIXTURE_PLAN] });
    }
    if (url.searchParams.get("chat") === "1") return Response.json({ plans: [] });
    if (url.pathname === "/api/plans") return Response.json({ plans: [entry] });
    if (url.pathname === `/api/plans/${FIXTURE_PLAN.id}/tasks`) {
      pageCalls++;
      return Response.json({
        plan_id: FIXTURE_PLAN.id,
        counts: { pending: 0, in_progress: 0, in_review: 0, blocked: 0, done: 0, cancelled: 0, total: 0 },
        columns: Object.fromEntries(["pending", "in_progress", "in_review", "blocked", "done"].map((status) => [status, { status, items: [], next_cursor: null }])),
      });
    }
    return Response.json({ plans: [] });
  };
  const ui = (
    <PlansProvider api={api}>
      <PlanViewShell
        mode="focus"
        plans={[]}
        planIndex={[entry]}
        api={api}
        planId={FIXTURE_PLAN.id}
        tabs={{}}
      />
    </PlansProvider>
  );
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
  const selectedMode = host.querySelector('[data-testid="plan-view-shell"]')?.getAttribute("data-plan-view-mode");
  assert(selectedMode === "tasks", `Tasks mode selected (got ${selectedMode ?? "none"})`);
  assert(pageCalls === 1, `one bounded task page requested (got ${pageCalls}; requests: ${requestPaths.join(", ")}; board=${Boolean(host.querySelector('[data-testid="plan-board-view"]'))}, single=${Boolean(host.querySelector('[data-testid="plan-board-single"]'))}, empty=${Boolean(host.querySelector('[data-testid="plan-board-empty"]'))}, loader=${Boolean(host.querySelector('[data-testid="plan-board-loading"]'))}, error=${Boolean(host.querySelector('[data-testid="plan-board-error"]'))})`);
  assert(fullCalls === 0, `Tasks mode did not request the full collection (got ${fullCalls})`);
  assert(host.querySelector('[data-testid="plan-board-columns"]'), "task board opened from the compact page");
  await act(async () => root.unmount());
  host.remove();
});

let passed = 0;
async function main() {
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(e);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} PlanViewShell tests passed`);
}

main();

test("useFrozenWhenHidden keeps last-visible reference while hidden", async () => {
  const { useFrozenWhenHidden } = await import("./PlanViewShell");
  const outputs: unknown[] = [];
  function Probe({ value, visible }: { value: { n: number }; visible: boolean }) {
    outputs.push(useFrozenWhenHidden(value, visible));
    return null;
  }
  const { createRoot } = await import("react-dom/client");
  const { createElement, act } = await import("react");
  const host = document.createElement("div");
  const root = createRoot(host);
  const a = { n: 1 };
  const b = { n: 2 };
  const c = { n: 3 };
  await act(async () => root.render(createElement(Probe, { value: a, visible: true })));
  await act(async () => root.render(createElement(Probe, { value: b, visible: false })));
  await act(async () => root.render(createElement(Probe, { value: c, visible: true })));
  if (outputs[0] !== a) throw new Error("visible passes live props");
  if (outputs[1] !== a) throw new Error("hidden must keep the last-visible reference (memo identity)");
  if (outputs[2] !== c) throw new Error("re-shown surface receives fresh props");
  await act(async () => root.unmount());
});
