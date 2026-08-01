// Run: npx tsx src/components/Canvas/planView/activeWorkStripVisibility.test.ts

import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan } from "../types";
import { selectActiveWork, type ActiveWorkEntry } from "./activeWork";
import { buildPlanListRows } from "./buildPlanListTree";
import {
  activeWorkEntriesForStrip,
  shouldShowActiveWorkStrip,
  visiblePlanListTaskIds,
} from "./activeWorkStripVisibility";
import { buildVisiblePlanListRows } from "./planListModel";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const tabs = {
  p1: { id: "p1", pty_id: null, tab_type: "Terminal", title: "Worker", cwd: null } as never,
};

function planWithTasks(tasks: Plan["tasks"]): Plan {
  return { ...FIXTURE_PLAN, tasks };
}

function visibleRowsFor(
  plan: Plan,
  opts: { statusFilter?: "all" | "active" | "done" | "blocked" | "cancelled"; textQuery?: string; collapsedSectionId?: string } = {},
) {
  const rows = buildPlanListRows(plan, { tabs });
  const expansionOverrides = new Map<string, boolean>();
  if (opts.collapsedSectionId) expansionOverrides.set(opts.collapsedSectionId, false);
  return buildVisiblePlanListRows(rows, {
    statusFilter: opts.statusFilter ?? "all",
    textQuery: opts.textQuery ?? "",
    expansionOverrides,
  });
}

function activeEntries(plan: Plan): readonly ActiveWorkEntry[] {
  return selectActiveWork([plan], tabs).entries;
}

const inflightPlan = planWithTasks([
  { ...FIXTURE_TASK, id: "t-wip", status: TaskStatus.InProgress, name: "WIP task" },
  { ...FIXTURE_TASK, id: "t-review", status: TaskStatus.InReview, name: "Review task" },
  { ...FIXTURE_TASK, id: "t-done", status: TaskStatus.Done, name: "Done task" },
]);

const defaultVisible = visibleRowsFor(inflightPlan);
const entries = activeEntries(inflightPlan);

assert(entries.length === 2, "two in-flight tasks");
assert(visiblePlanListTaskIds(defaultVisible).has("t-wip"), "wip visible in list");
assert(visiblePlanListTaskIds(defaultVisible).has("t-review"), "review visible in list");
assert(!shouldShowActiveWorkStrip(entries, defaultVisible), "default unfiltered list hides strip");
assert(activeWorkEntriesForStrip(entries, defaultVisible).length === 0, "no strip entries by default");

const doneFiltered = visibleRowsFor(inflightPlan, { statusFilter: "done" });
assert(shouldShowActiveWorkStrip(entries, doneFiltered), "done filter shows strip");
const stripWhenDone = activeWorkEntriesForStrip(entries, doneFiltered);
assert(stripWhenDone.length === 2, "strip carries hidden in-flight tasks");
assert(stripWhenDone.every((e) => e.task.id === "t-wip" || e.task.id === "t-review"), "strip ids match in-flight");

const searchFiltered = visibleRowsFor(inflightPlan, { textQuery: "review" });
assert(shouldShowActiveWorkStrip(entries, searchFiltered), "search hiding wip shows strip");
const stripWhenSearch = activeWorkEntriesForStrip(entries, searchFiltered);
assert(stripWhenSearch.length === 1 && stripWhenSearch[0]?.task.id === "t-wip", "only hidden wip in strip");
assert(visiblePlanListTaskIds(searchFiltered).has("t-review"), "review still in list");

const activeSectionId = `${inflightPlan.id}::status::active`;
const collapsed = visibleRowsFor(inflightPlan, { collapsedSectionId: activeSectionId });
assert(shouldShowActiveWorkStrip(entries, collapsed), "collapsed active group shows strip");
assert(activeWorkEntriesForStrip(entries, collapsed).length === 2, "collapsed hides all in-flight in list");

const visibleWhenCollapsed = visiblePlanListTaskIds(collapsed);
const stripWhenCollapsed = activeWorkEntriesForStrip(entries, collapsed);
for (const entry of entries) {
  const inList = visibleWhenCollapsed.has(entry.task.id);
  const inStrip = stripWhenCollapsed.some((e) => e.task.id === entry.task.id);
  assert(inList !== inStrip, `xor for ${entry.task.id}`);
}

assert(!shouldShowActiveWorkStrip([], defaultVisible), "empty entries never show strip");

console.log("activeWorkStripVisibility: all assertions passed");
