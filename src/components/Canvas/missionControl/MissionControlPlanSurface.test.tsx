// Run: npx tsx src/components/Canvas/missionControl/MissionControlPlanSurface.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { Workspace } from "../../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task, type TaskGroup } from "../types";
import { MissionControlPlanSurface } from "./MissionControlPlanSurface";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/mc-surface" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  (window as unknown as { matchMedia: (q: string) => unknown }).matchMedia = () => ({
    matches: false,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
  });
  class ResizeObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  (globalThis as { ResizeObserver?: unknown }).ResizeObserver = ResizeObserverStub;
  (window as unknown as { ResizeObserver: unknown }).ResizeObserver = ResizeObserverStub;
  if (typeof (window as unknown as { requestAnimationFrame?: unknown }).requestAnimationFrame !== "function") {
    (window as unknown as { requestAnimationFrame: (cb: () => void) => number }).requestAnimationFrame = () => 0;
    (window as unknown as { cancelAnimationFrame: () => void }).cancelAnimationFrame = () => {};
  }
  for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "Node", "requestAnimationFrame", "cancelAnimationFrame"]) {
    const value = (window as unknown as Record<string, unknown>)[key];
    if (value !== undefined) Object.defineProperty(globalThis, key, { value, configurable: true });
  }
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
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

const GROUPS: TaskGroup[] = [
  { key: "wave-26", label: "Wave 26", color: null },
  { key: "wave-27", label: "Wave 27", color: null },
];

// FIXTURE_PLAN owner is orch-1 → bind it to the current workspace for scoping.
const WS_ID = "ws-test";
const WORKSPACES: Workspace[] = [
  { id: WS_ID, name: "Test", tabs: ["orch-1"], active_tab_id: null, cwd: null },
];

function mkTask(id: string, status: TaskStatus, group: string): Task {
  return { ...FIXTURE_TASK, id, name: id, status, group, section_ids: [], milestones: [] };
}

function mkPlan(): Plan {
  return {
    ...FIXTURE_PLAN,
    id: "pl_flow",
    title: "Plan views UX redesign",
    task_groups: GROUPS,
    loops: [],
    tasks: [
      mkTask("t1", TaskStatus.InProgress, "wave-26"),
      mkTask("t2", TaskStatus.Pending, "wave-26"),
      mkTask("t3", TaskStatus.Done, "wave-26"),
      mkTask("t4", TaskStatus.Pending, "wave-27"),
    ],
  };
}

test("renders the selected plan board without the removed status band", async () => {
  const { host, unmount } = await render(
    <MissionControlPlanSurface mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[mkPlan()]} />,
  );
  assert(host.querySelector('[data-testid="mission-control-board"]'), "board rendered");
  assert(!host.querySelector('[data-testid="plan-board-status"]'), "status header removed");
  const header = host.querySelector('[data-testid="mission-control-surface-header"]');
  assert(header, "surface header rendered");
  const trigger = header.querySelector('[data-testid="mission-control-board-menu-trigger"]') as HTMLButtonElement | null;
  assert(trigger?.tagName === "BUTTON", "board actions live in the surface header");
  assert(trigger.tabIndex === 0, "board actions are keyboard reachable");
  assert(
    !host.querySelector('[data-testid="mission-control-board"] [data-testid="mission-control-board-actions"]'),
    "the board itself carries no actions band",
  );
  assert(!trigger.closest('[data-testid="plan-board-status"]'), "board actions are outside status band");
  await act(async () => (trigger as HTMLButtonElement).click());
  const labels = Array.from(host.querySelectorAll('[data-testid="mission-control-board-menu"] [role="menuitem"]'))
    .map((item) => item.textContent?.trim());
  assert(labels.join("|") === "Approve plan|Pause plan|Clear everything", "board actions remain available");
  assert(
    host.querySelector('[data-testid="mission-control-board-actions"]')?.getAttribute("data-plan-id") === "pl_flow",
    "board actions bind to the selected plan",
  );
  await unmount();
});

test("waits for the stored reading instead of inventing card figures", async () => {
  const { host, unmount } = await render(
    <MissionControlPlanSurface mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[mkPlan()]} />,
  );
  assert(host.querySelector('[data-testid="mc-board-pending"]'), "pending state without an api config");
  assert(!host.querySelector('[data-testid="mc-board-row"]'), "no cards without a snapshot");
  await unmount();
});

test("workspace surface has no all-plans escape hatch", async () => {
  const { host, unmount } = await render(
    <MissionControlPlanSurface
      mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES}
      plans={[mkPlan()]}
    />,
  );
  assert(!host.querySelector('[data-testid="mc-scope-all-plans"]'), "all-plans control is absent");
  await unmount();
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed += 1;
      console.error(`  ✗ ${name}: ${(e as Error).message}`);
    }
  }
  if (failed > 0) {
    console.error(`\n${failed} of ${tests.length} surface test(s) failed`);
    process.exit(1);
  }
  console.log(`\n✓ all ${tests.length} surface test(s) green`);
}

void main();
