// Run: npx tsx src/components/Canvas/planView/TaskActivityTimeline.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_MILESTONE, FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import type { Milestone, Task, TaskExecutionAttempt } from "../types";
import { TaskActivityTimeline } from "./TaskActivityTimeline.tsx";
import { milestoneTimestampLabel } from "./taskActivityModel";

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
  const window = new Window({ url: "http://localhost/task-activity" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
}

setupDom();

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
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

function milestone(id: string, kind: string, ts: number, text: string): Milestone {
  return {
    ...FIXTURE_MILESTONE,
    id,
    kind,
    ts,
    text,
    from: { tab_id: "tab-a", session_id: null, title: "Puccini" },
  };
}

test("hides activity section when task has no milestones or attempts", async () => {
  const { host } = await render(<TaskActivityTimeline task={FIXTURE_TASK} plan={FIXTURE_PLAN} />);
  assert(!host.querySelector('[data-testid="plan-task-activity"]'), "empty task hides activity");
});

test("renders milestone timeline with kind, age, and author", async () => {
  const now = 1_700_000_060_000;
  const task: Task = {
    ...FIXTURE_TASK,
    milestones: [milestone("ms-1", "status_update", 1_700_000_000_000, "Shipped first pass")],
  };
  const { host } = await render(
    <TaskActivityTimeline task={task} plan={FIXTURE_PLAN} now={now} tabs={{ "tab-a": { id: "tab-a", title: "Puccini" } as never }} />,
  );
  const entry = host.querySelector('[data-testid="plan-task-activity-entry"]');
  assert(entry, "timeline entry rendered");
  assert(host.querySelector('[data-testid="plan-task-activity-kind"]')?.textContent?.includes("status update"), "kind chip");
  assert(host.querySelector('[data-testid="plan-task-activity-age"]')?.textContent === "1m", "relative age");
  const stamp = host.querySelector('[data-testid="plan-task-activity-timestamp"]');
  assert(stamp?.textContent === milestoneTimestampLabel(1_700_000_000_000, now), "absolute date+time stamp");
  const kindEl = host.querySelector('[data-testid="plan-task-activity-kind"]');
  const ageEl = host.querySelector('[data-testid="plan-task-activity-age"]');
  const FOLLOWING = 4;
  assert(
    kindEl && stamp && ageEl &&
      (kindEl.compareDocumentPosition(stamp) & FOLLOWING) !== 0 &&
      (stamp.compareDocumentPosition(ageEl) & FOLLOWING) !== 0,
    "meta order: kind → absolute stamp → relative age",
  );
  assert(host.querySelector('[data-testid="plan-task-activity-author"]')?.textContent === "Puccini", "author label");
  assert(entry?.textContent?.includes("Shipped first pass"), "stride text");
});

test("collapses long milestone history and expands on show-all", async () => {
  const task: Task = {
    ...FIXTURE_TASK,
    milestones: Array.from({ length: 7 }, (_, i) => milestone(`ms-${i}`, "testing", 1_700_000_000_000 + i * 1000, `step ${i}`)),
  };
  const { host } = await render(<TaskActivityTimeline task={task} plan={FIXTURE_PLAN} now={1_700_000_500_000} />);
  assert(host.querySelectorAll('[data-testid="plan-task-activity-entry"]').length === 5, "collapsed to five");
  const showAll = host.querySelector('[data-testid="plan-task-activity-show-all"]') as HTMLButtonElement;
  assert(showAll?.textContent?.includes("Show all 7"), "show-all label");
  await act(async () => {
    showAll.click();
  });
  assert(host.querySelectorAll('[data-testid="plan-task-activity-entry"]').length === 7, "expanded to all");
  assert(host.querySelector('[data-testid="plan-task-activity-timeline"]')?.getAttribute("data-expanded") === "true", "expanded flag");
});

test("renders only this task milestones", async () => {
  const task: Task = {
    ...FIXTURE_TASK,
    milestones: [milestone("mine", "decision", 1_700_000_000_000, "only mine")],
  };
  const otherTask: Task = {
    ...FIXTURE_TASK,
    id: "task-other",
    milestones: [milestone("theirs", "review", 1_700_000_100_000, "not mine")],
  };
  const plan = { ...FIXTURE_PLAN, tasks: [task, otherTask] };
  const { host } = await render(<TaskActivityTimeline task={task} plan={plan} now={1_700_000_200_000} />);
  const ids = Array.from(host.querySelectorAll('[data-testid="plan-task-activity-entry"]'))
    .map((el) => el.getAttribute("data-milestone-id"));
  assert(ids.join(",") === "mine", "only current task milestones");
});

test("renders grouped execution attempts", async () => {
  const attempts: TaskExecutionAttempt[] = [
    { id: "a1", tab_id: "tab-a", tab_title: "Worker A", role: "frontend-dev", model: "gpt", branch: "lane/a", report_status: "in_review" },
    { id: "a2", tab_id: "tab-b", tab_title: "Worker B", role: "backend-dev", model: "claude", branch: "lane/b", report_status: "blocked" },
  ];
  const task: Task = { ...FIXTURE_TASK, execution_attempts: attempts, milestones: [] };
  const { host } = await render(<TaskActivityTimeline task={task} plan={FIXTURE_PLAN} />);
  assert(host.querySelector('[data-testid="plan-task-activity-attempts"]'), "attempts block");
  assert(host.querySelectorAll('[data-testid="plan-task-activity-attempt"]').length === 2, "two attempts");
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
  console.log(`\n✓ ${passed}/${tests.length} TaskActivityTimeline tests passed`);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
