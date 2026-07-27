// Run: npx tsx src/components/Canvas/planView/PlanListView.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { Workspace } from "../../../lib/types";
import { FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { buildPerfFixturePlans } from "../perfFixture";
import { TaskStatus, type Plan } from "../types";
import { DONE_LIST_FILTER_VISIBLE_LIMIT } from "./planListModel";
import { PlanListView } from "./PlanListView";

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
  Object.defineProperty(globalThis, "PointerEvent", { value: window.PointerEvent, configurable: true });
  (window as unknown as { confirm: (msg: string) => boolean }).confirm = () => true;
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

// FIXTURE_PLAN owner is orch-1; perf-fixture plans own perf_orch — bind both.
const WS_ID = "ws-test";
const WORKSPACES: Workspace[] = [
  {
    id: WS_ID,
    name: "Test",
    tabs: ["orch-1", "perf_orch"],
    active_tab_id: null,
    cwd: null,
  },
];

test("renders plan root and task rows with stable test ids", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [FIXTURE_TASK],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  assert(host.querySelector('[data-testid="plan-list-view"]'), "root");
  assert(host.querySelector('[data-testid="plan-list-scroll"]'), "scroll region");
  assert(host.querySelector('[data-testid="plan-list-filter"]'), "filter toolbar");
  assert(host.querySelector('[data-testid="plan-context-header"]'), "context header");
  assert(!host.querySelector(".flex.flex-1.flex-col.items-center.justify-center"), "no dead-band spacer");
  assert(host.querySelector('[data-testid="plan-board-plan-title"]'), "single plan title chip");
  assert(!host.querySelector('[data-testid="plan-list-plan-root"]'), "no duplicated plan-root band");
  const task = host.querySelector('[data-testid="plan-list-task"]');
  assert(task, "task row");
  assert(task?.getAttribute("data-task-id") === FIXTURE_TASK.id, "task id hook");
  assert(host.querySelector('[data-testid="plan-list-belonging-chip"]')?.textContent === "Authentication", "belonging chip");
  assert(host.textContent?.includes("Auth rollout"), "plan title");
  assert(host.textContent?.includes("Dante"), "task name");
  await unmount();
});

test("spec opens via header chip, not a spec tree in the list", async () => {
  const [plan] = buildPerfFixturePlans(8);
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  assert(host.querySelector('[data-testid="plan-board-spec-title"]'), "spec chip in context header");
  assert(!host.querySelector('[data-testid="plan-list-spec-root"]'), "no spec root band in list");
  assert(!host.querySelector('[data-testid="plan-list-spec-section"]'), "no spec sections in list");
  await unmount();
});

test("empty state when no active plans", async () => {
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[]} />);
  assert(host.querySelector('[data-testid="plan-list-empty"]'), "empty state");
  await unmount();
});

test("task title is a single-line cell with full name preserved for tooltip", async () => {
  const longName = "Implement resilient offline queue sync for mesh peers across flaky networks";
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [{ ...FIXTURE_TASK, name: longName }],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const row = host.querySelector('[data-testid="plan-list-task"]') as HTMLElement | null;
  const title = host.querySelector('[data-testid="plan-list-task"] [data-testid="plan-list-title"]') as HTMLElement | null;
  assert(title?.className.includes("plan-list-title"), "alignment-grid title cell");
  assert(row?.getAttribute("title")?.includes(longName), "full name available as tooltip");
  assert(host.textContent?.includes(longName), "full task name in DOM");
  await unmount();
});

test("done-only section defaults to rollup instead of task rows", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [{ ...FIXTURE_SECTION, id: "sec-done", title: "Shipped" }],
    tasks: [
      { ...FIXTURE_TASK, id: "t-done-1", status: TaskStatus.Done, section_ids: ["sec-done"] },
      { ...FIXTURE_TASK, id: "t-done-2", status: TaskStatus.Done, name: "Also done", section_ids: ["sec-done"] },
    ],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  assert(host.querySelector('[data-testid="plan-list-done-rollup"]'), "rollup row");
  assert(!host.querySelector('[data-testid="plan-list-task"]'), "no expanded done tasks");
  assert(host.textContent?.includes("2 done"), "rollup count");
  await unmount();
});

test("section collapse toggle expands hidden tasks", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [{ ...FIXTURE_SECTION, id: "sec-done", title: "Shipped" }],
    tasks: [{ ...FIXTURE_TASK, id: "t-done-1", status: TaskStatus.Done, section_ids: ["sec-done"] }],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const section = host.querySelector('[data-testid="plan-list-plan-section"]') as HTMLElement;
  assert(section, "section row");
  await act(async () => {
    section.click();
  });
  assert(host.querySelector('[data-testid="plan-list-task"]'), "task visible after expand");
  await unmount();
});

test("status filter chip limits visible tasks", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [
      { ...FIXTURE_TASK, id: "t-active", status: TaskStatus.InProgress },
      { ...FIXTURE_TASK, id: "t-done", status: TaskStatus.Done, name: "Finished" },
    ],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const doneChip = host.querySelector('[data-testid="plan-list-filter-status-done"]') as HTMLElement;
  await act(async () => {
    doneChip.click();
  });
  const tasks = host.querySelectorAll('[data-testid="plan-list-task"]');
  assert(tasks.length === 1, `one done task visible, got ${tasks.length}`);
  assert(tasks[0]?.getAttribute("data-task-id") === "t-done", "done task id");
  await unmount();
});

test("paused tasks stay in pending group with paused chip and reason title", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [
      {
        ...FIXTURE_TASK,
        id: "t-paused",
        status: TaskStatus.Paused,
        name: "Deferred lane",
        pause_reason: "Provider quotas exhausted",
      },
    ],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const group = host.querySelector('[data-testid="plan-list-plan-section"]') as HTMLElement;
  assert(group?.textContent?.includes("Pending"), "paused task grouped under pending");
  const row = host.querySelector('[data-testid="plan-list-task"]') as HTMLElement;
  assert(row?.getAttribute("data-task-id") === "t-paused", "paused task visible");
  assert(row.className.includes("plan-list-row-paused"), "paused row style");
  assert(row.getAttribute("title") === "Paused: Provider quotas exhausted", "pause reason title");
  assert(row.querySelector('[data-testid="plan-list-paused-chip"]')?.textContent === "paused", "paused chip");
  await unmount();
});

test("clicking a task row opens PlanDetailOverlay", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [{ ...FIXTURE_TASK, id: "t-open", status: TaskStatus.Pending, name: "Open me" }],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const row = host.querySelector('[data-testid="plan-list-task"]') as HTMLElement;
  assert(row, "task row");
  await act(async () => {
    row.click();
  });
  assert(host.querySelector('[data-testid="plan-detail-overlay"]'), "overlay open");
  assert(host.querySelector('[data-testid="plan-detail-overlay"]')?.getAttribute("data-detail-mode") === "slide-over", "task uses slide-over");
  assert(host.querySelector('[data-testid="plan-detail-kind"]')?.textContent === "task", "task kind");
  await unmount();
});

test("clicking context header plan title opens plan detail overlay", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [{ ...FIXTURE_TASK, status: TaskStatus.Pending }],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const planTitle = host.querySelector('[data-testid="plan-board-plan-title"]') as HTMLElement;
  await act(async () => {
    planTitle.click();
  });
  assert(host.querySelector('[data-testid="plan-detail-overlay"]'), "overlay open");
  assert(host.querySelector('[data-testid="plan-detail-overlay"]')?.getAttribute("data-detail-mode") === "document", "plan uses document overlay");
  assert(host.querySelector('[data-testid="plan-detail-kind"]')?.textContent === "plan", "plan kind");
  await unmount();
});

test("clicking a row belonging chip opens plan detail overlay", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [{ ...FIXTURE_TASK, id: "t-chip", status: TaskStatus.Pending, name: "Chip target" }],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const chip = host.querySelector('[data-testid="plan-list-belonging-chip"]') as HTMLElement;
  assert(chip, "belonging chip");
  await act(async () => {
    chip.click();
  });
  assert(host.querySelector('[data-testid="plan-detail-overlay"]'), "overlay open");
  assert(host.querySelector('[data-testid="plan-detail-kind"]')?.textContent === "plan", "plan kind");
  await unmount();
});

test("cancelled filter chip shows only cancelled tasks", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [
      { ...FIXTURE_TASK, id: "t-active", status: TaskStatus.InProgress },
      { ...FIXTURE_TASK, id: "t-cancelled", status: TaskStatus.Cancelled, name: "Dropped work", handle: "drop-it" },
    ],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const chip = host.querySelector('[data-testid="plan-list-filter-status-cancelled"]') as HTMLElement;
  assert(chip?.textContent?.includes("Cancelled (1)"), "cancelled chip with count");
  await act(async () => {
    chip.click();
  });
  const tasks = host.querySelectorAll('[data-testid="plan-list-task"]');
  assert(tasks.length === 1, `one cancelled task visible, got ${tasks.length}`);
  assert(tasks[0]?.getAttribute("data-task-id") === "t-cancelled", "cancelled task id");
  assert(host.textContent?.includes("Dropped work"), "cancelled title visible");
  await unmount();
});

test("task row is selectable and suppresses open on drag", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [{ ...FIXTURE_TASK, id: "t-drag", status: TaskStatus.Pending, name: "Drag test" }],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const row = host.querySelector('[data-testid="plan-list-task"]') as HTMLElement;
  assert(row?.getAttribute("role") === "button", "task row remains activatable");
  assert(row.className.includes("plan-task-selectable"), "selectable class");
  await act(async () => {
    row.dispatchEvent(new PointerEvent("pointerdown", { clientX: 0, clientY: 0, bubbles: true }));
    row.dispatchEvent(new PointerEvent("pointermove", { clientX: 24, clientY: 0, bubbles: true }));
    row.click();
  });
  assert(!host.querySelector('[data-testid="plan-detail-overlay"]'), "drag does not open detail");
  await unmount();
});

test("list: archive action removes plan from active switcher and switches selection", async () => {
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
    tasks: [{ ...FIXTURE_TASK, id: "a1", status: TaskStatus.Pending }],
  };
  const planB: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-b",
    title: "Beta",
    status: "active",
    created_at: 0,
    updated_at: 999,
    tasks: [{ ...FIXTURE_TASK, id: "b1", status: TaskStatus.Pending, name: "Beta task" }],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[planA, planB]} />);
  assert(host.querySelector('[data-testid="plan-switcher"]'), "switcher visible");

  const kebab = host.querySelector(
    '[data-testid="plan-switcher-kebab"][data-plan-id="plan-b"]',
  ) as HTMLElement;
  await act(async () => { kebab.click(); });
  const archiveItem = Array.from(host.querySelectorAll('[role="menuitem"]')).find(
    (el) => el.textContent?.includes("Archive"),
  ) as HTMLElement;
  await act(async () => { archiveItem.click(); });
  await act(async () => { await Promise.resolve(); });

  const activePills = host.querySelectorAll('[data-testid="plan-switcher-option"]');
  const bPill = Array.from(activePills).find(
    (el) => el.getAttribute("data-plan-id") === "plan-b",
  );
  assert(!bPill, "plan-b no longer active pill");

  const toggle = host.querySelector('[data-testid="plan-switcher-archived-toggle"]') as HTMLElement;
  assert(toggle, "archived toggle appears");
  assert(toggle.textContent?.includes("1 archived"), "count = 1");

  await unmount();
});

test("list: unarchive brings plan back to active switcher", async () => {
  const active: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-active",
    title: "Active",
    status: "active",
    tasks: [{ ...FIXTURE_TASK, id: "t1", status: TaskStatus.Pending }],
  };
  const archived: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-arch",
    title: "Archived",
    status: "archived",
    tasks: [],
  };
  const { host, unmount } = await render(
    <PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[active, archived]} />,
  );
  const toggle = host.querySelector('[data-testid="plan-switcher-archived-toggle"]') as HTMLElement;
  assert(toggle, "archived toggle present");
  await act(async () => { toggle.click(); });

  const kebab = host.querySelector(
    '[data-testid="plan-switcher-kebab"][data-plan-id="plan-arch"]',
  ) as HTMLElement;
  assert(kebab, "kebab on archived plan");
  await act(async () => { kebab.click(); });

  const unarchive = Array.from(host.querySelectorAll('[role="menuitem"]')).find(
    (el) => el.textContent?.trim() === "Unarchive",
  ) as HTMLElement;
  await act(async () => { unarchive.click(); });
  await act(async () => { await Promise.resolve(); });

  const pill = host.querySelector(
    '[data-testid="plan-switcher-option"][data-plan-id="plan-arch"]',
  );
  assert(pill, "plan-arch is now an active pill after unarchive");

  await unmount();
});

test("default list hides in-flight strip when status group shows tasks", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [{ ...FIXTURE_TASK, id: "t-wip", status: TaskStatus.InProgress, name: "WIP task" }],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  assert(host.querySelector('[data-testid="plan-list-task"][data-task-id="t-wip"]'), "in-flight task in list");
  assert(!host.querySelector('[data-testid="active-work-strip"]'), "strip hidden when list shows in-flight");
  await unmount();
});

test("done filter shows in-flight strip for hidden active tasks", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [
      { ...FIXTURE_TASK, id: "t-wip", status: TaskStatus.InProgress, name: "WIP task" },
      { ...FIXTURE_TASK, id: "t-done", status: TaskStatus.Done, name: "Done task" },
    ],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const doneChip = host.querySelector('[data-testid="plan-list-filter-status-done"]') as HTMLElement;
  await act(async () => {
    doneChip.click();
  });
  assert(!host.querySelector('[data-testid="plan-list-task"][data-task-id="t-wip"]'), "wip hidden in list");
  const stripRow = host.querySelector('[data-testid="active-work-strip-row"][data-task-id="t-wip"]');
  assert(stripRow, "wip visible in strip");
  await unmount();
});

test("collapsed in progress section shows in-flight strip", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [{ ...FIXTURE_TASK, id: "t-wip", status: TaskStatus.InProgress, name: "WIP task" }],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const section = host.querySelector('[data-testid="plan-list-plan-section"]') as HTMLElement;
  assert(section?.textContent?.includes("In progress + review"), "active status group");
  await act(async () => {
    section.click();
  });
  assert(!host.querySelector('[data-testid="plan-list-task"][data-task-id="t-wip"]'), "wip hidden when collapsed");
  assert(host.querySelector('[data-testid="active-work-strip-row"][data-task-id="t-wip"]'), "wip in strip");
  await unmount();
});

test("done filter caps heavy done plans with show-all toggle", async () => {
  const tasks = Array.from({ length: 250 }, (_, i) => ({
    ...FIXTURE_TASK,
    id: `t-done-${i}`,
    name: `Done ${i}`,
    status: TaskStatus.Done,
    status_changed_at: 250 - i,
    updated_at: 250 - i,
  }));
  const plan: Plan = { ...FIXTURE_PLAN, tasks };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const doneChip = host.querySelector('[data-testid="plan-list-filter-status-done"]') as HTMLElement;
  await act(async () => {
    doneChip.click();
  });
  const visibleTasks = host.querySelectorAll('[data-testid="plan-list-task"]');
  assert(
    visibleTasks.length === DONE_LIST_FILTER_VISIBLE_LIMIT,
    `done filter capped to ${DONE_LIST_FILTER_VISIBLE_LIMIT}, got ${visibleTasks.length}`,
  );
  const toggle = host.querySelector('[data-testid="plan-list-done-filter-toggle"]') as HTMLElement;
  assert(toggle?.textContent?.includes("Show all 250"), "show-all toggle label");
  await act(async () => {
    toggle.click();
  });
  assert(
    host.querySelectorAll('[data-testid="plan-list-task"]').length === 250,
    "show-all reveals every done task",
  );
  await unmount();
});

test("in-flight strip click opens task detail when strip is visible", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    sections: [FIXTURE_SECTION],
    tasks: [
      { ...FIXTURE_TASK, id: "t-wip", status: TaskStatus.InProgress, name: "Strip open me" },
      { ...FIXTURE_TASK, id: "t-done", status: TaskStatus.Done, name: "Done task" },
    ],
  };
  const { host, unmount } = await render(<PlanListView mode="workspace" workspaceId={WS_ID} workspaces={WORKSPACES} plans={[plan]} />);
  const doneChip = host.querySelector('[data-testid="plan-list-filter-status-done"]') as HTMLElement;
  await act(async () => {
    doneChip.click();
  });
  const stripRow = host.querySelector('[data-testid="active-work-strip-row"]') as HTMLElement;
  assert(stripRow, "strip row visible");
  await act(async () => {
    stripRow.click();
  });
  assert(host.querySelector('[data-testid="plan-detail-overlay"]'), "overlay open from strip");
  assert(host.querySelector('[data-testid="plan-detail-kind"]')?.textContent === "task", "task detail");
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
