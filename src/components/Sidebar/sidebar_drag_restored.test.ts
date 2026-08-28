import type { DragEndEvent } from "@dnd-kit/core";
import { isTabDragDisabled } from "./SidebarPlanPeerPair";
import {
  POINTER_DRAG_ACTIVATION_DISTANCE,
  commitSidebarDrag,
  type ReorderHandlers,
  type SortableGroupData,
} from "./sortable";

type TestFn = () => void;

const tests: Array<[string, TestFn]> = [];
const test = (name: string, fn: TestFn) => tests.push([name, fn]);

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function assertDeepEqual(actual: unknown, expected: unknown, message: string): void {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  assert(actualJson === expectedJson, `${message}: ${actualJson} !== ${expectedJson}`);
}

interface DragFixture {
  group: SortableGroupData["group"];
  groupKey: string;
  items: string[];
  activeId?: string;
  overId?: string;
  pinnedIds?: string[];
  overGroup?: SortableGroupData["group"];
  overGroupKey?: string;
  keyboard?: boolean;
}

function dragEvent({
  group,
  groupKey,
  items,
  activeId = items[0],
  overId = items[1],
  pinnedIds = [],
  overGroup = group,
  overGroupKey = groupKey,
  keyboard = false,
}: DragFixture): DragEndEvent {
  const data = (itemGroup: SortableGroupData["group"], itemGroupKey: string) => ({
    group: itemGroup,
    groupKey: itemGroupKey,
    pinnedIds,
    items: [],
    sortable: { containerId: `${itemGroup}:${itemGroupKey}`, items },
  });
  return {
    active: {
      id: activeId,
      data: { current: data(group, groupKey) },
    },
    over: {
      id: overId,
      data: { current: data(overGroup, overGroupKey) },
    },
    activatorEvent: keyboard ? new Event("keydown") : new Event("pointerdown"),
  } as unknown as DragEndEvent;
}

function recordingHandlers() {
  const workspaceCalls: string[][] = [];
  const tabCalls: Parameters<ReorderHandlers["reorderTabs"]>[] = [];
  const handlers: ReorderHandlers = {
    reorderWorkspaces: (ordered) => workspaceCalls.push(ordered),
    reorderTabs: (args) => tabCalls.push([args]),
  };
  return { handlers, workspaceCalls, tabCalls };
}

test("sidebar_drag_restored workspace cards reorder through the workspace handler", () => {
  const recorded = recordingHandlers();
  commitSidebarDrag(dragEvent({
    group: "workspace",
    groupKey: "workspaces",
    items: ["ws:first", "ws:second"],
  }), recorded.handlers);
  assertDeepEqual(recorded.workspaceCalls, [["second", "first"]], "workspace order");
});

for (const surface of ["manager", "worker", "terminal"] as const) {
  test(`sidebar_drag_restored ${surface} rows reorder through the workspace tab handler`, () => {
    const recorded = recordingHandlers();
    commitSidebarDrag(dragEvent({
      group: "tab",
      groupKey: "workspace",
      items: [`tab:${surface}-one`, `tab:${surface}-two`],
    }), recorded.handlers);
    assertDeepEqual(recorded.tabCalls, [[{
      workspaceId: "workspace",
      orderedTabIds: [`${surface}-two`, `${surface}-one`],
    }]], `${surface} order`);
  });
}

test("sidebar_drag_restored child rows reorder through the parent tab handler", () => {
  const recorded = recordingHandlers();
  commitSidebarDrag(dragEvent({
    group: "child",
    groupKey: "parent",
    items: ["tab:child-one", "tab:child-two"],
  }), recorded.handlers);
  assertDeepEqual(recorded.tabCalls, [[{
    parentTabId: "parent",
    orderedTabIds: ["child-two", "child-one"],
  }]], "child order");
});

test("sidebar_drag_restored cross-group drops remain refused", () => {
  const recorded = recordingHandlers();
  commitSidebarDrag(dragEvent({
    group: "tab",
    groupKey: "workspace-one",
    overGroupKey: "workspace-two",
    items: ["tab:first", "tab:second"],
  }), recorded.handlers);
  assert(recorded.tabCalls.length === 0, "cross-group drop must not commit");
});

test("sidebar_drag_restored pinned prefix remains ahead of General", () => {
  const recorded = recordingHandlers();
  commitSidebarDrag(dragEvent({
    group: "tab",
    groupKey: "workspace",
    items: ["tab:general", "tab:first", "tab:second"],
    activeId: "tab:second",
    overId: "tab:general",
    pinnedIds: ["tab:general"],
  }), recorded.handlers);
  assert(recorded.tabCalls.length === 0, "drop above General must not commit");
});

test("sidebar_drag_restored protected rows remain disabled", () => {
  assert(isTabDragDisabled({ protected: true }), "protected row must be disabled");
  assert(!isTabDragDisabled({ protected: false }), "ordinary row must remain draggable");
});

test("sidebar_drag_restored sub-threshold pointer movement remains a click", () => {
  assert(3 < POINTER_DRAG_ACTIVATION_DISTANCE, "three-pixel movement must not activate drag");
});

test("sidebar_drag_restored keyboard drop reaches the same reorder handler", () => {
  const recorded = recordingHandlers();
  commitSidebarDrag(dragEvent({
    group: "tab",
    groupKey: "workspace",
    items: ["tab:first", "tab:second"],
    keyboard: true,
  }), recorded.handlers);
  assertDeepEqual(recorded.tabCalls, [[{
    workspaceId: "workspace",
    orderedTabIds: ["second", "first"],
  }]], "keyboard order");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (error) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(error);
  }
}

if (failed > 0) process.exit(1);
console.log(`\n✓ ${tests.length}/${tests.length} sidebar_drag_restored tests passed`);
