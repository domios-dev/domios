// Run: npx tsx src/components/Canvas/planView/BoardTaskCard.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task } from "../types";
import { buildBoardPlanModel } from "./boardModel";
import { BoardTaskCard } from "./BoardTaskCard";

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
  const window = new Window({ url: "http://localhost/board-card" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
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
  return { host, root };
}

function task(overrides: Partial<Task>): Task {
  return { ...FIXTURE_TASK, ...overrides };
}

function cardFor(plan: Plan, taskId: string) {
  const model = buildBoardPlanModel(plan, {});
  for (const column of model.columns) {
    const card = column.cards.find((entry) => entry.task.id === taskId);
    if (card) return card;
  }
  throw new Error(`card not found for ${taskId}`);
}

test("status chip renders canonical status only", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "t-status",
        status: TaskStatus.InProgress,
        status_label: "Active: QA Pipeline Mechanics",
      }),
    ],
  };
  const { host } = await render(<BoardTaskCard card={cardFor(plan, "t-status")} />);
  const chip = host.querySelector('[data-testid="status-chip"]');
  assert(chip?.textContent === "in progress", "canonical status text");
  assert(!host.querySelector('[data-testid="latest-note"]'), "no note without milestones");
});

test("latest note is separate from status chip for decision milestones", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "t-note",
        status: TaskStatus.InReview,
        status_label: "In QA · queued #1",
        review_stage: "qa",
        qa_state: "queued",
        milestones: [
          {
            id: "ms-old",
            from: { pane_id: "p1", session_id: null, title: "Orchestrator" },
            kind: "status_update",
            text: "older update",
            ts: 1_000,
            links: [],
            loop_id: null,
            iteration: null,
            canvas: { x: null, y: null, collapsed: false },
          },
          {
            id: "ms-new",
            from: { pane_id: "p2", session_id: null, title: "Worker" },
            kind: "decision",
            text: "Demote intent text to muted latest-note line",
            ts: 9_000,
            links: [],
            loop_id: null,
            iteration: null,
            canvas: { x: null, y: null, collapsed: false },
          },
        ],
      }),
    ],
  };
  const { host } = await render(<BoardTaskCard card={cardFor(plan, "t-note")} />);
  const chip = host.querySelector('[data-testid="status-chip"]');
  const note = host.querySelector('[data-testid="latest-note"]');
  assert(chip?.textContent === "in review", "status chip stays canonical");
  assert(note?.textContent === "Demote intent text to muted latest-note line", "latest milestone text");
  assert(chip !== note, "status chip and latest note are distinct nodes");
  const ordered = Array.from(host.querySelectorAll("[data-testid]")).map((el) => el.getAttribute("data-testid"));
  assert(ordered.indexOf("status-chip") < ordered.indexOf("latest-note"), "latest note follows status chip");
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
  console.log(`\n${passed}/${tests.length} passed`);
})();
