// Run: npx tsx src/components/Canvas/planView/PlanListView.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { buildPerfFixturePlans } from "../perfFixture";
import { PlanListView } from "./PlanListView";
import type { Plan } from "../types";

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
  const window = new Window({ url: "http://localhost/plan-list-view" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
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
      await act(async () => {
        root.unmount();
      });
      host.remove();
    },
  };
}

test("renders plan root and task rows with stable test ids", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [FIXTURE_TASK],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" plans={[plan]} />);
  assert(host.querySelector('[data-testid="plan-list-view"]'), "root");
  assert(host.querySelector('[data-testid="plan-list-scroll"]'), "scroll region");
  assert(host.querySelector('[data-testid="plan-list-plan-root"]'), "plan entity band");
  const task = host.querySelector('[data-testid="plan-list-task"]');
  assert(task, "task row");
  assert(task?.getAttribute("data-task-id") === FIXTURE_TASK.id, "task id hook");
  assert(host.textContent?.includes("Auth rollout"), "plan title");
  assert(host.textContent?.includes("Dante"), "task name");
  await unmount();
});

test("shows spec entity band when spec sections are supplied via fixture plan", async () => {
  const [plan] = buildPerfFixturePlans(8);
  const { host, unmount } = await render(<PlanListView mode="workspace" plans={[plan]} />);
  assert(host.querySelector('[data-testid="plan-list-spec-root"]'), "spec entity");
  assert((host.querySelectorAll('[data-testid="plan-list-spec-section"]').length ?? 0) > 0, "spec sections");
  await unmount();
});

test("empty state when no active plans", async () => {
  const { host, unmount } = await render(<PlanListView mode="workspace" plans={[]} />);
  assert(host.querySelector('[data-testid="plan-list-empty"]'), "empty state");
  await unmount();
});

test("task title uses wrap-friendly class, not single-line truncate", async () => {
  const longName = "Implement resilient offline queue sync for mesh peers across flaky networks";
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [{ ...FIXTURE_TASK, name: longName }],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" plans={[plan]} />);
  const title = host.querySelector('[data-testid="plan-list-title"]') as HTMLElement | null;
  assert(title?.className.includes("break-words"), "wrap-friendly title");
  assert(!title?.className.includes("truncate"), "no truncate on task title");
  assert(host.textContent?.includes(longName), "full task name visible");
  await unmount();
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
  console.log(`\n✓ ${passed}/${tests.length} PlanListView tests passed`);
}

main();
