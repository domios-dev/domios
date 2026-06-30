// Run: npx tsx src/components/Canvas/planView/PlanBoardView.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task } from "../types";
import { PlanBoardView } from "./PlanBoardView";

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
  const window = new Window({ url: "http://localhost/plan-board" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
}

setupDom();

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  host.style.width = "1200px";
  host.style.height = "800px";
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  await act(async () => {
    await Promise.resolve();
  });
  return { host, root };
}

function task(overrides: Partial<Task>): Task {
  return { ...FIXTURE_TASK, ...overrides };
}

const PANES = {
  "pane-worker": { id: "pane-worker", title: "Puccini" } as never,
};

test("renders loading gate", async () => {
  const { host } = await render(
    <PlanBoardView mode="workspace" plans={[]} loading error={null} />,
  );
  assert(host.querySelector('[data-testid="plan-board-loading"]'), "loading visible");
});

test("renders board columns with spec/plan summaries and task cards", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    primary_spec_id: "spec-fixture",
    heartbeat: { current_focus: "task:task-dante" },
    goal: { vector: "Deliver auth" },
    tasks: [
      task({
        id: "task-dante",
        status: TaskStatus.InProgress,
        assigned_pane_id: "pane-worker",
        lane: { branch: "lane/plan-board-view", worktree_path: "/tmp/codeterm-plan-board-view" },
        milestones: [
          {
            id: "ms-1",
            from: { pane_id: "pane-worker", session_id: null, title: "Puccini" },
            kind: "status_update",
            text: "Board model landed",
            ts: 2,
            links: [],
            loop_id: null,
            iteration: null,
            canvas: { x: null, y: null, collapsed: false },
          },
        ],
      }),
      task({ id: "task-wait", status: TaskStatus.Pending, name: "Queue next" }),
    ],
  };
  const { host } = await render(
    <PlanBoardView
      mode="focus"
      planId={plan.id}
      plans={[plan]}
      panes={PANES}
    />,
  );
  assert(host.querySelector('[data-testid="plan-board-view"]'), "board root");
  assert(host.querySelector('[data-testid="plan-board-spec-summary"]'), "spec summary");
  assert(host.querySelector('[data-testid="plan-board-plan-summary"]'), "plan summary");
  assert(host.querySelector('[data-testid="plan-board-plan-goal"]')?.textContent?.includes("Deliver auth"), "goal");
  const columns = host.querySelectorAll('[data-testid="plan-board-column"]');
  assert(columns.length === 5, `column count=${columns.length}`);
  const inProgress = host.querySelector('[data-testid="plan-board-column"][data-column="in_progress"]');
  const card = inProgress?.querySelector('[data-testid="plan-board-task-card"]');
  assert(card, "in-progress card");
  assert(card?.getAttribute("data-task-id") === "task-dante", "task id");
  assert(host.querySelector('[data-testid="plan-board-task-agent"]')?.textContent?.includes("Puccini"), "agent");
  assert(host.querySelector('[data-testid="plan-board-task-lane"]')?.textContent?.includes("lane/plan-board-view"), "lane");
  assert(host.querySelector('[data-testid="plan-board-task-milestone"]')?.textContent?.includes("Board model landed"), "milestone");
});

test("empty workspace shows stable empty gate", async () => {
  const { host } = await render(<PlanBoardView mode="workspace" plans={[]} />);
  assert(host.querySelector('[data-testid="plan-board-empty"]'), "empty state");
});

let passed = 0;
(async () => {
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
  console.log(`\n✓ ${passed}/${tests.length} PlanBoardView tests passed`);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
