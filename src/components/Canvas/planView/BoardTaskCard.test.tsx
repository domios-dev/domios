import { FIXTURE_BOARD_SUMMARY } from "../cards/fixtures.testdata";
// Run: npx tsx src/components/Canvas/planView/BoardTaskCard.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type PlanTaskBoardSummary, type Task } from "../types";
import { buildBoardPlanModel, buildSummaryBoardTaskCard } from "./boardModel";
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
        status_label: "blocked: reviewer host unavailable overnight",
        review_stage: "qa",
        qa_state: "queued",
        milestones: [
          {
            id: "ms-old",
            from: { tab_id: "p1", session_id: null, title: "Orchestrator" },
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
            from: { tab_id: "p2", session_id: null, title: "Worker" },
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
  assert(
    chip?.textContent === "In QA · queued #1 — blocked: reviewer host unavailable overnight",
    "status chip states the queue state and the operator reason, not bare 'in review'",
  );
  assert(note?.textContent === "Demote intent text to muted latest-note line", "latest milestone text");
  assert(chip !== note, "status chip and latest note are distinct nodes");
  const ordered = Array.from(host.querySelectorAll("[data-testid]")).map((el) => el.getAttribute("data-testid"));
  assert(ordered.indexOf("status-chip") < ordered.indexOf("latest-note"), "latest note follows status chip");
});

test("queued QA chip degrades honestly when the operator left no reason", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "t-queued-no-label",
        status: TaskStatus.InReview,
        status_label: null,
        review_stage: "qa",
        qa_state: "queued",
      }),
    ],
  };
  const { host } = await render(<BoardTaskCard card={cardFor(plan, "t-queued-no-label")} />);
  const chip = host.querySelector('[data-testid="status-chip"]');
  assert(chip?.textContent === "In QA · queued #1", "queued state shown without inventing a reason");
});

test("done card with stale tab id never renders tab-gone", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "t-done-ghost",
        status: TaskStatus.Done,
        assigned_tab_id: "ghost-tab",
        lane: { branch: "lane/merged" },
        status_changed_at: 1_000,
      }),
    ],
  };
  const { host } = await render(<BoardTaskCard card={cardFor(plan, "t-done-ghost")} />);
  assert(!host.querySelector('[data-testid="working-on-row"]'), "no working-on on done");
  assert(!host.textContent?.includes("tab gone"), "no tab-gone text");
  assert(host.querySelector('[data-testid="plan-board-task-lane"]'), "branch row on done");
  assert(host.querySelector('[data-testid="plan-board-task-age"]'), "age row on done");
});

test("locator renders the task group and its lead section (what + where)", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    task_groups: [{ key: "planner-cli-dx", label: "planner-cli-dx", color: "#a371f7" }],
    tasks: [
      task({ id: "t-loc", group: "planner-cli-dx", section_ids: ["sec-auth"] }),
    ],
  };
  const { host } = await render(<BoardTaskCard card={cardFor(plan, "t-loc")} />);
  const loc = host.querySelector('[data-testid="plan-board-task-locator"]');
  assert(loc, "locator present when group/section exist");
  const group = host.querySelector('[data-testid="plan-board-task-locator-group"]');
  assert(group?.textContent?.includes("planner-cli-dx"), "group label shown");
  const section = host.querySelector('[data-testid="plan-board-task-locator-section"]');
  assert(section?.textContent?.includes("Authentication"), "lead section title shown");
});

test("locator is absent when a task has neither group nor section", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [task({ id: "t-bare", group: null, section_ids: [] })],
  };
  const { host } = await render(<BoardTaskCard card={cardFor(plan, "t-bare")} />);
  assert(!host.querySelector('[data-testid="plan-board-task-locator"]'), "no locator without group/section");
});

test("bounded summary card keeps locator, execution badges, historical executor, age and status metadata", async () => {
  const summary: PlanTaskBoardSummary = {
    ...FIXTURE_BOARD_SUMMARY,
    id: "summary-done",
    position: 8,
    name: "Summarized completed task",
    status: TaskStatus.Done,
    created_at: 1_000,
    updated_at: 2_000,
    status_changed_at: 2_000,
    activity_at: 2_500,
    key: "CT-88",
    locator: { groupKey: "release", groupLabel: "Release", groupColor: "#a371f7", section: "Packaging" },
    lane: { tab_id: null, branch: "release/1.18" },
    latest_note: "Release check completed",
    historical_executor: {
      tab_id: "tab-worker",
      tab_title: "Puccini",
      role: "release-engineer",
      model: "gpt-6.1-sol",
      branch: "release/1.18",
      report_status: "merged",
      outcome: "done",
    },
    competition_badge: { count: 2, live_count: 0, judge_hint: "best result", models: ["model-a", "model-b"], live_attempts: [] },
    loop_badge: {
      count: 3,
      budget: 5,
      status: "running",
      stopWhen: "checks pass",
      warning: false,
      tooltip: "loop history is summarized",
      loops: [],
    },
  };
  const { host } = await render(
    <BoardTaskCard card={buildSummaryBoardTaskCard(summary, { "tab-worker": { id: "tab-worker", title: "Puccini" } as never }, 10_000)} />,
  );
  assert(host.querySelector('[data-testid="plan-board-task-locator-group"]')?.textContent?.includes("Release"), "group locator retained");
  assert(host.querySelector('[data-testid="plan-board-task-locator-section"]')?.textContent?.includes("Packaging"), "section locator retained");
  assert(host.querySelector('[data-testid="task-loop-badge"]')?.textContent?.includes("3"), "loop badge retained");
  assert(host.querySelector('[data-testid="task-compete-badge"]')?.textContent?.includes("2"), "competition badge retained");
  assert(host.querySelector('[data-testid="plan-board-task-historical-executor"]')?.textContent?.includes("Puccini"), "historical executor retained");
  assert(host.querySelector('[data-testid="plan-board-task-age"]'), "age metadata retained");
  assert(host.querySelector('[data-testid="plan-board-task-lane"]')?.textContent?.includes("release/1.18"), "lane metadata retained");
  assert(host.querySelector('[data-testid="status-chip"]')?.textContent === "done", "canonical status retained");
  assert(host.querySelector('[data-testid="latest-note"]')?.textContent === "Release check completed", "bounded latest note retained");
});

test("bounded active summary keeps its working-on model, tab, branch and age chips", async () => {
  const summary: PlanTaskBoardSummary = {
    ...FIXTURE_BOARD_SUMMARY,
    id: "summary-active",
    position: 9,
    name: "Active summarized task",
    status: TaskStatus.InProgress,
    created_at: 1_000,
    updated_at: 8_000,
    status_changed_at: 7_000,
    activity_at: 8_500,
    assigned_tab_id: "tab-worker",
    model: "gpt-6.1-sol",
    peer: "build-peer",
    lane: { tab_id: "tab-worker", branch: "feature/summary" },
  };
  const { host } = await render(
    <BoardTaskCard card={buildSummaryBoardTaskCard(summary, { "tab-worker": { id: "tab-worker", title: "Puccini" } as never }, 10_000)} />,
  );
  assert(host.querySelector('[data-testid="working-on-row"]'), "working-on row retained");
  assert(host.querySelector('[data-testid="working-on-chip-model"]')?.textContent?.includes("gpt-6.1-sol"), "model chip retained");
  assert(host.querySelector('[data-testid="working-on-chip-tab"]')?.textContent?.includes("Puccini"), "tab chip retained");
  assert(host.querySelector('[data-testid="working-on-chip-peer"]')?.textContent?.includes("build-peer"), "peer chip retained");
  assert(host.querySelector('[data-testid="working-on-chip-branch"]')?.textContent?.includes("feature/summary"), "branch chip retained");
  assert(host.querySelector('[data-testid="working-on-chip-age"]'), "working age retained");
});

test("bounded summary preserves QA queue position and operator status reason", () => {
  const card = buildSummaryBoardTaskCard({
    ...FIXTURE_BOARD_SUMMARY,
    id: "qa-summary",
    position: 4,
    name: "Queued QA task",
    status: TaskStatus.InReview,
    created_at: 1,
    updated_at: 2,
    status_changed_at: 2,
    review_stage: "qa",
    qa_state: "queued",
    qa_queue_position: 3,
    status_label: "waiting for reviewer capacity",
  }, {}, 3);
  assert(card.statusChipText === "In QA · queued #3 — waiting for reviewer capacity", "QA queue and operator label retained");
});

test("interactive card exposes its task action to assistive technology", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [task({ id: "t-accessible", name: "Repair board layout" })],
  };
  const { host } = await render(
    <BoardTaskCard card={cardFor(plan, "t-accessible")} onOpen={() => {}} />,
  );
  const card = host.querySelector('[data-testid="plan-board-task-card"]');
  assert(card?.getAttribute("role") === "button", "interactive card role");
  assert(card?.getAttribute("aria-label") === "Open task T-ACCESSIBLE Repair board layout", "task action name");
});

test("card names the task by its key before the title, with the legacy id fallback", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({ id: "task_keyed1", key: "CT-12", name: "Keyed task" }),
      task({ id: "task_legacy9", key: null, name: "Legacy task" }),
    ],
  };
  const { host } = await render(
    <>
      <BoardTaskCard card={cardFor(plan, "task_keyed1")} onOpen={() => {}} />
      <BoardTaskCard card={cardFor(plan, "task_legacy9")} onOpen={() => {}} />
    </>,
  );
  const cards = Array.from(host.querySelectorAll('[data-testid="plan-board-task-card"]'));
  const read = (card: Element) => [
    card.querySelector('[data-testid="plan-board-task-key"]')?.textContent,
    card.querySelector('[data-testid="plan-board-task-title"]')?.textContent,
    card.getAttribute("aria-label"),
  ];
  assert(JSON.stringify(read(cards[0]!)) === JSON.stringify(["CT-12", "Keyed task", "Open task CT-12 Keyed task"]), "keyed card");
  assert(JSON.stringify(read(cards[1]!)) === JSON.stringify(["LEGACY9", "Legacy task", "Open task LEGACY9 Legacy task"]), "legacy card");
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
