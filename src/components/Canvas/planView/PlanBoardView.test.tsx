// Run: npx tsx src/components/Canvas/planView/PlanBoardView.test.tsx

import { act, type ReactElement } from "react";
import { readFileSync } from "node:fs";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { Workspace } from "../../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task } from "../types";
import { DONE_COLUMN_COLLAPSED_LIMIT } from "./boardModel";
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
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "PointerEvent", { value: window.PointerEvent, configurable: true });
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

const TABS = {
  "tab-worker": { id: "tab-worker", title: "Puccini" } as never,
};

// FIXTURE_PLAN.owner.tab_id is orch-1 → bind it to this test workspace.
const WS_ID = "ws-test";
const WORKSPACES: Workspace[] = [
  { id: WS_ID, name: "Test", tabs: ["orch-1"], active_tab_id: null, cwd: null },
];

test("renders loading gate", async () => {
  const { host } = await render(
    <PlanBoardView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[]} loading error={null} />,
  );
  assert(host.querySelector('[data-testid="plan-board-loading"]'), "loading visible");
});

test("renders board columns without the repeated status header and with task cards", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    primary_spec_id: "spec-fixture",
    planning_state: "approved",
    planner: { kind: "tab", tab_id: "planner-1", title: "Planner" },
    heartbeat: { current_focus: "task:task-dante" },
    goal: { vector: "Deliver auth" },
    tasks: [
      task({
        id: "task-dante",
        status: TaskStatus.InProgress,
        assigned_tab_id: "tab-worker",
        lane: { branch: "lane/plan-board-view", worktree_path: "/tmp/codeterm-plan-board-view" },
        milestones: [
          {
            id: "ms-1",
            from: { tab_id: "tab-worker", session_id: null, title: "Puccini" },
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
      tabs={TABS}
    />,
  );
  assert(host.querySelector('[data-testid="plan-board-view"]'), "board root");
  assert(!host.querySelector('[data-testid="plan-board-status"]'), "status header removed");
  assert(!host.querySelector('[data-testid="plan-board-spec-title"]'), "spec summary removed from board");
  assert(!host.querySelector('[data-testid="plan-board-plan-goal"]'), "goal summary removed from board");
  const planBoard = host.querySelector('[data-testid="plan-board-plan"]');
  assert(planBoard?.firstElementChild?.getAttribute("data-testid") === "plan-board-columns", "columns start the task surface");
  const columns = host.querySelectorAll('[data-testid="plan-board-column"]');
  assert(columns.length === 5, `column count=${columns.length}`);
  assert(
    Array.from(columns).map((column) => column.querySelector('[data-testid="plan-board-column-label"]')?.textContent?.trim().toUpperCase()).join("|") ===
      "PENDING|IN PROGRESS|IN REVIEW|BLOCKED|DONE",
    "all kanban headings remain",
  );
  const inProgress = host.querySelector('[data-testid="plan-board-column"][data-column="in_progress"]');
  const card = inProgress?.querySelector('[data-testid="plan-board-task-card"]');
  assert(card, "in-progress card");
  assert(card?.getAttribute("data-task-id") === "task-dante", "task id");
  assert(host.querySelector('[data-testid="working-on-chip-tab"]')?.textContent?.includes("Puccini"), "working-on Tab chip");
  const laneText = host.querySelector('[data-testid="working-on-chip-branch"]')?.textContent ?? "";
  assert(laneText.includes("lane/plan-board-view"), "working-on branch chip");
  assert(!host.querySelector('[data-testid="plan-board-task-milestone"]'), "no raw milestone card row");
  assert(host.querySelector('[data-testid="latest-note"]')?.textContent?.includes("Board model landed"), "latest note present");
  assert(!host.querySelector('[data-testid="plan-board-single"]')?.innerHTML.includes("/tmp/codeterm-plan-board-view"), "no worktree path leaked to DOM");
});

test("single-plan switcher: no vertical stacking, switch selects one plan", async () => {
  const planA: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-a",
    title: "Alpha rollout",
    created_at: 0,
    updated_at: 10,
    tasks: [task({ id: "a1", status: TaskStatus.Pending, name: "Alpha task", created_at: 0, updated_at: 10 })],
  };
  const planB: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-b",
    title: "Beta rollout",
    created_at: 0,
    updated_at: 999,
    tasks: [task({ id: "b1", status: TaskStatus.Pending, name: "Beta task", created_at: 0, updated_at: 999 })],
  };
  const { host } = await render(<PlanBoardView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[planA, planB]} tabs={TABS} />);
  const boards = host.querySelectorAll('[data-testid="plan-board-plan"]');
  assert(boards.length === 1, `single board rendered, got ${boards.length}`);
  assert(host.querySelector('[data-testid="plan-switcher"]'), "switcher present for >1 plan");
  assert(host.querySelector('[data-testid="plan-board-plan"]')?.getAttribute("data-plan-id") === "plan-b", "freshest plan default");
  const options = host.querySelectorAll('[data-testid="plan-switcher-option"]');
  const alpha = Array.from(options).find((o) => o.getAttribute("data-plan-id") === "plan-a") as HTMLElement;
  assert(alpha, "alpha option");
  await act(async () => {
    alpha.click();
  });
  assert(host.querySelector('[data-testid="plan-board-plan"]')?.getAttribute("data-plan-id") === "plan-a", "manual selection applied");
  assert(host.querySelectorAll('[data-testid="plan-board-plan"]').length === 1, "still single board after switch");
});

test("switcher hidden for a single plan", async () => {
  const plan: Plan = { ...FIXTURE_PLAN, tasks: [task({ id: "solo", status: TaskStatus.Pending })] };
  const { host } = await render(<PlanBoardView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} tabs={TABS} />);
  assert(!host.querySelector('[data-testid="plan-switcher"]'), "no switcher for single plan");
  assert(host.querySelectorAll('[data-testid="plan-board-plan"]').length === 1, "single board");
});

test("clicking a task card opens PlanDetailOverlay", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [task({ id: "task-open", status: TaskStatus.Pending, name: "Open me" })],
  };
  const { host } = await render(<PlanBoardView mode="focus" planId={plan.id} plans={[plan]} tabs={TABS} />);
  const card = host.querySelector('[data-testid="plan-board-task-card"]') as HTMLElement;
  assert(card, "task card");
  await act(async () => {
    card.click();
  });
  assert(host.querySelector('[data-testid="plan-detail-overlay"]'), "overlay open");
  assert(host.querySelector('[data-testid="plan-detail-overlay"]')?.getAttribute("data-detail-mode") === "slide-over", "task uses slide-over");
  assert(host.querySelector('[data-testid="plan-detail-kind"]')?.textContent === "task", "task kind");
});

test("clicking another task updates the open slide-over panel", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({ id: "task-first", status: TaskStatus.Pending, name: "First task" }),
      task({ id: "task-second", status: TaskStatus.Pending, name: "Second task" }),
    ],
  };
  const { host } = await render(<PlanBoardView mode="focus" planId={plan.id} plans={[plan]} tabs={TABS} />);
  const cards = Array.from(host.querySelectorAll('[data-testid="plan-board-task-card"]')) as HTMLElement[];
  await act(async () => {
    cards[0]?.click();
  });
  const panel = host.querySelector('[data-testid="plan-detail-panel"]');
  assert(host.querySelector('[data-testid="plan-detail-title"]')?.textContent === "First task", "first task title");
  await act(async () => {
    cards[1]?.click();
  });
  assert(host.querySelector('[data-testid="plan-detail-panel"]') === panel, "same panel element reused");
  assert(host.querySelector('[data-testid="plan-detail-title"]')?.textContent === "Second task", "second task title");
});

test("done column collapses to recent N with show-all toggle", async () => {
  const doneTasks = Array.from({ length: DONE_COLUMN_COLLAPSED_LIMIT + 3 }, (_, i) =>
    task({ id: `done-${i}`, status: TaskStatus.Done, status_changed_at: 100 - i, name: `Done ${i}` }),
  );
  const plan: Plan = { ...FIXTURE_PLAN, tasks: doneTasks };
  const { host } = await render(<PlanBoardView mode="focus" planId={plan.id} plans={[plan]} tabs={TABS} />);
  const doneCol = host.querySelector('[data-testid="plan-board-column"][data-column="done"]');
  const cards = doneCol?.querySelectorAll('[data-testid="plan-board-task-card"]');
  assert(cards?.length === DONE_COLUMN_COLLAPSED_LIMIT, `collapsed cards=${cards?.length}`);
  const toggle = doneCol?.querySelector('[data-testid="plan-board-done-toggle"]') as HTMLElement;
  assert(toggle?.textContent?.includes(`Show all ${doneTasks.length}`), "show all label");
  await act(async () => {
    toggle.click();
  });
  const expandedCards = doneCol?.querySelectorAll('[data-testid="plan-board-task-card"]');
  assert(expandedCards?.length === doneTasks.length, `expanded cards=${expandedCards?.length}`);
});

test("paused tasks render in pending with status and reason title", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "task-paused",
        status: TaskStatus.Paused,
        name: "Deferred lane",
        pause_reason: "Provider quotas exhausted",
      }),
      task({ id: "task-pending", status: TaskStatus.Pending, name: "Queue next" }),
    ],
  };
  const { host } = await render(<PlanBoardView mode="focus" planId={plan.id} plans={[plan]} tabs={TABS} />);
  const pendingCol = host.querySelector('[data-testid="plan-board-column"][data-column="pending"]');
  assert(pendingCol, "pending column");
  assert(pendingCol?.querySelector('[data-testid="plan-board-column-count"]')?.textContent === "2", "pending includes paused count");
  assert(!host.querySelector('[data-testid="plan-board-column"][data-column="paused"]'), "paused column removed");
  const pausedCard = pendingCol?.querySelector('[data-task-id="task-paused"]') as HTMLElement;
  assert(pausedCard, "paused card in pending column");
  assert(pausedCard.className.includes("plan-board-task-card-paused"), "paused card style");
  assert(pausedCard.getAttribute("title") === "Paused: Provider quotas exhausted", "pause reason title");
  assert(pausedCard.querySelector('[data-testid="status-chip"]')?.textContent === "paused", "paused status chip");
});

test("done task card shows historical executor without active executor row", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "task-done",
        status: TaskStatus.Done,
        assigned_tab_id: "tab-worker",
        taken_by_role: "backend-dev",
        model: "gpt-5.5",
      }),
    ],
  };
  const { host } = await render(<PlanBoardView mode="focus" planId={plan.id} plans={[plan]} tabs={TABS} />);
  const doneCol = host.querySelector('[data-testid="plan-board-column"][data-column="done"]');
  const card = doneCol?.querySelector('[data-testid="plan-board-task-card"]');
  assert(card, "done card");
  assert(!card?.querySelector('[data-testid="plan-board-task-active-executor"]'), "no active executor");
  assert(card?.querySelector('[data-testid="plan-board-task-historical-executor"]')?.textContent?.includes("Puccini"), "historical executor");
});

test("task card text is selectable and drag does not open detail", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [task({ id: "task-select", status: TaskStatus.Pending, name: "Copy me" })],
  };
  const { host } = await render(<PlanBoardView mode="focus" planId={plan.id} plans={[plan]} tabs={TABS} />);
  const card = host.querySelector('[data-testid="plan-board-task-card"]') as HTMLElement;
  assert(card.className.includes("plan-task-selectable"), "selectable class");

  await act(async () => {
    card.dispatchEvent(new PointerEvent("pointerdown", { clientX: 0, clientY: 0, bubbles: true }));
    card.dispatchEvent(new PointerEvent("pointermove", { clientX: 20, clientY: 0, bubbles: true }));
    card.click();
  });
  assert(!host.querySelector('[data-testid="plan-detail-overlay"]'), "drag click suppressed");
});

test("clicking cancelled note reveals cancelled tasks with titles", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({ id: "task-active", status: TaskStatus.Pending, name: "Active" }),
      task({ id: "task-cancelled", status: TaskStatus.Cancelled, name: "Scrapped auth", handle: "scrap-auth" }),
    ],
  };
  const { host } = await render(<PlanBoardView mode="focus" planId={plan.id} plans={[plan]} tabs={TABS} />);
  const note = host.querySelector('[data-testid="plan-board-cancelled-note"]') as HTMLElement;
  assert(note?.tagName === "BUTTON", "cancelled note is clickable");
  await act(async () => {
    note.click();
  });
  const row = host.querySelector('[data-testid="plan-cancelled-task-row"]') as HTMLElement;
  assert(row, "cancelled row visible");
  assert(row.getAttribute("data-task-id") === "task-cancelled", "task id preserved");
  assert(host.textContent?.includes("Scrapped auth"), "title shown");
  assert(host.textContent?.includes("scrap-auth"), "handle shown");
  await act(async () => {
    row.click();
  });
  assert(host.querySelector('[data-testid="plan-detail-overlay"]'), "cancelled task opens detail");
});

test("task card: status row precedes branch row, long branch has truncate class", async () => {
  const longBranch = "feat/very-long-branch-name-that-could-overflow-the-task-card-layout-significantly";
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "task-layout",
        status: TaskStatus.InProgress,
        lane: { branch: longBranch, worktree_path: null },
      }),
    ],
  };
  const { host } = await render(
    <PlanBoardView mode="focus" planId={plan.id} plans={[plan]} tabs={TABS} />,
  );
  const card = host.querySelector('[data-testid="plan-board-task-card"]');
  assert(card, "card present");
  const status = card?.querySelector('[data-testid="status-chip"]') as Element;
  const lane = card?.querySelector('[data-testid="working-on-chip-branch"]') as Element;
  assert(status, "status present");
  assert(lane, "working-on branch chip present");
  assert(lane.textContent?.includes("Branch"), "branch field label present");
  assert(lane.querySelector(".working-on-chip-value")?.textContent?.includes("…"), "long branch middle-truncated");
  const allWithTestId = Array.from(card!.querySelectorAll("[data-testid]"));
  const statusIdx = allWithTestId.indexOf(status);
  const laneIdx = allWithTestId.indexOf(lane);
  assert(statusIdx !== -1 && laneIdx !== -1, "both elements found in testid scan");
  assert(statusIdx < laneIdx, `status (idx ${statusIdx}) must precede branch (idx ${laneIdx}) in DOM order`);
  const branchSpan = lane.querySelector(".working-on-chip-value");
  assert(branchSpan, "branch value span carries truncate class for ellipsis clipping");
});

test("task card: title stays primary and does not allow single-letter wrapping", async () => {
  const longTitle = "zero-flag-plan-context";
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      task({
        id: "task-title-primary",
        status: TaskStatus.InProgress,
        status_label: "Active: QA Pipeline Mechanics",
        name: longTitle,
        handle: "zero-flag-plan-context",
      }),
    ],
  };
  const { host } = await render(
    <PlanBoardView mode="focus" planId={plan.id} plans={[plan]} tabs={TABS} />,
  );
  const card = host.querySelector('[data-testid="plan-board-task-card"]');
  assert(card, "card present");
  const title = card.querySelector('[data-testid="plan-board-task-title"]') as HTMLElement;
  const status = card.querySelector('[data-testid="status-chip"]') as HTMLElement;
  // working tasks replace the raw #handle noise with the labeled Working-on row.
  const workingOn = card.querySelector('[data-testid="working-on-row"]') as HTMLElement;
  assert(title?.textContent === longTitle, "full task title is present");
  assert(status?.textContent === "in progress", "status chip uses canonical task.status only");
  assert(!card.querySelector('[data-testid="plan-board-task-handle"]'), "raw # handle replaced on working card");
  assert(workingOn, "working-on row present on working card");
  assert(
    Boolean(title.compareDocumentPosition(status) & Node.DOCUMENT_POSITION_FOLLOWING),
    "title must precede the status pill",
  );
  assert(
    Boolean(title.compareDocumentPosition(workingOn) & Node.DOCUMENT_POSITION_FOLLOWING),
    "title must precede the working-on row",
  );
  const css = readFileSync("src/styles/globals.css", "utf8");
  const titleRule = css.match(/\.plan-board-task-title\s*\{[^}]+\}/)?.[0] ?? "";
  assert(!titleRule.includes("overflow-wrap: anywhere"), "title must not break into single-letter wraps");
  assert(titleRule.includes("word-break: normal"), "title keeps normal word boundaries");
});

test("empty workspace shows stable empty gate", async () => {
  const { host } = await render(<PlanBoardView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[]} />);
  assert(host.querySelector('[data-testid="plan-board-empty"]'), "empty state");
});

test("mobile board CSS enables horizontal snap columns and 44px touch targets", () => {
  const css = readFileSync("src/styles/globals.css", "utf8");
  const mobileBlock = css.match(/@media \(max-width: 768px\), \(pointer: coarse\)\s*\{[\s\S]*?\n\}/)?.[0] ?? "";
  assert(mobileBlock.includes("scroll-snap-type: x mandatory"), "column snap scroll");
  assert(mobileBlock.includes(".plan-board-column-head") && mobileBlock.includes("min-height: 44px"), "sticky column heads");
  assert(mobileBlock.includes(".plan-board-task-card-interactive") && mobileBlock.includes("min-height: 44px"), "card touch targets");
  const cardRule = css.match(/\.plan-board-task-card\s*\{[^}]+\}/)?.[0] ?? "";
  assert(cardRule.includes("flex: 0 0 auto"), "cards keep their content height inside scroll columns");
  assert(cardRule.includes("overflow: hidden"), "card metadata cannot bleed into adjacent cards");
  const valueRule = css.match(/\.plan-board-task-meta-value\s*\{[^}]+\}/)?.[0] ?? "";
  assert(valueRule.includes("text-overflow: ellipsis"), "narrow metadata values use ellipsis");
  assert(valueRule.includes("white-space: nowrap"), "metadata values remain within one row");
});

test("board: archive action removes plan from active switcher and switches selection", async () => {
  try {
    window.sessionStorage.removeItem("codeterm.board.selectedPlanId");
  } catch {
    // Session persistence is best-effort in tests too.
  }
  const planA: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-a",
    title: "Alpha",
    status: "active",
    created_at: 0,
    updated_at: 10,
    tasks: [task({ id: "a1", status: TaskStatus.Pending, name: "Alpha task" })],
  };
  const planB: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-b",
    title: "Beta",
    status: "active",
    created_at: 0,
    updated_at: 999,
    tasks: [task({ id: "b1", status: TaskStatus.Pending, name: "Beta task" })],
  };
  const { host } = await render(
    <PlanBoardView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[planA, planB]} tabs={TABS} />,
  );
  // Stub confirm on the window created by setupDom (which runs once at module level).
  (window as unknown as { confirm: () => boolean }).confirm = () => true;
  // Switcher shows for >1 plan; Beta is default (most recent)
  assert(host.querySelector('[data-testid="plan-switcher"]'), "switcher visible");
  assert(
    host.querySelector('[data-testid="plan-board-plan"]')?.getAttribute("data-plan-id") === "plan-b",
    "beta selected by default",
  );

  const kebab = host.querySelector(
    '[data-testid="plan-switcher-kebab"][data-plan-id="plan-b"]',
  ) as HTMLElement;
  await act(async () => { kebab.click(); });
  const archiveItem = Array.from(host.querySelectorAll('[role="menuitem"]')).find(
    (el) => el.textContent?.includes("Archive"),
  ) as HTMLElement;
  await act(async () => { archiveItem.click(); });

  // plan-b should no longer be the board plan (selection fell back to plan-a)
  await act(async () => { await Promise.resolve(); });
  assert(
    host.querySelector('[data-testid="plan-board-plan"]')?.getAttribute("data-plan-id") === "plan-a",
    "selection fell back to plan-a after archiving plan-b",
  );

  const activePills = host.querySelectorAll('[data-testid="plan-switcher-option"]');
  const planBPill = Array.from(activePills).find(
    (el) => el.getAttribute("data-plan-id") === "plan-b",
  );
  assert(!planBPill, "plan-b no longer an active pill");

  const archivedToggle = host.querySelector('[data-testid="plan-switcher-archived-toggle"]') as HTMLElement;
  assert(archivedToggle, "archived toggle present after archive");
  await act(async () => { archivedToggle.click(); });
  const archivedRow = host.querySelector(
    '[data-testid="plan-switcher-archived-list"] [data-plan-id="plan-b"]',
  );
  assert(archivedRow, "plan-b appears in archived footer");
});

test("board: unarchive brings plan back to active switcher", async () => {
  const active: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-active",
    title: "Active",
    status: "active",
    tasks: [task({ id: "t1", status: TaskStatus.Pending })],
  };
  const archived: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-arch",
    title: "Archived",
    status: "archived",
    tasks: [],
  };
  const { host } = await render(
    <PlanBoardView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[active, archived]} tabs={TABS} />,
  );
  // Archived toggle should be present (1 active + 1 archived = switcher shows)
  const archivedToggle = host.querySelector(
    '[data-testid="plan-switcher-archived-toggle"]',
  ) as HTMLElement;
  assert(archivedToggle, "archived toggle present");
  await act(async () => { archivedToggle.click(); });

  const kebab = host.querySelector(
    '[data-testid="plan-switcher-kebab"][data-plan-id="plan-arch"]',
  ) as HTMLElement;
  assert(kebab, "kebab on archived plan");
  await act(async () => { kebab.click(); });

  const unarchive = Array.from(host.querySelectorAll('[role="menuitem"]')).find(
    (el) => el.textContent?.trim() === "Unarchive",
  ) as HTMLElement;
  assert(unarchive, "Unarchive menu item");
  await act(async () => { unarchive.click(); });
  await act(async () => { await Promise.resolve(); });

  const pill = host.querySelector(
    '[data-testid="plan-switcher-option"][data-plan-id="plan-arch"]',
  );
  assert(pill, "plan-arch is now an active pill after unarchive");
});

test("board: selecting an archived plan from dropdown shows it for inspection", async () => {
  try {
    window.sessionStorage.removeItem("codeterm.board.selectedPlanId");
  } catch {
    // Session persistence is best-effort in tests too.
  }
  const active: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-active",
    title: "Active",
    status: "active",
    created_at: 0,
    updated_at: 10,
    tasks: [task({ id: "active-task", status: TaskStatus.Pending, name: "Active task" })],
  };
  const archived: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-arch",
    title: "Archived",
    status: "archived",
    created_at: 0,
    updated_at: 999,
    tasks: [task({ id: "archived-task", status: TaskStatus.Pending, name: "Archived task" })],
  };
  const { host } = await render(
    <PlanBoardView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[active, archived]} tabs={TABS} />,
  );
  assert(
    host.querySelector('[data-testid="plan-board-plan"]')?.getAttribute("data-plan-id") === "plan-active",
    "active plan remains the default visible plan",
  );

  const toggle = host.querySelector('[data-testid="plan-switcher-archived-toggle"]') as HTMLElement;
  assert(toggle, "archived dropdown present");
  await act(async () => { toggle.click(); });
  const archivedSelect = host.querySelector(
    '[data-testid="plan-switcher-archived-list"] [data-plan-id="plan-arch"] .plan-switcher-opt-select',
  ) as HTMLElement;
  assert(archivedSelect, "archived plan is selectable");
  await act(async () => { archivedSelect.click(); });

  assert(
    host.querySelector('[data-testid="plan-board-plan"]')?.getAttribute("data-plan-id") === "plan-arch",
    "archived plan is shown for inspection",
  );
  assert(host.textContent?.includes("Archived task"), "archived plan content visible");
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
