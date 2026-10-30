// Run: npx tsx src/components/Terminal/dsTasks/dsTasksModel.test.ts

import { parseDsTasksMessage } from "@codeterm/chat-ui/dsTasksBlock";
import { FIXTURE_MILESTONE, FIXTURE_TASK } from "../../Canvas/cards/fixtures.testdata";
import type { Plan, Task } from "../../Canvas/types";
import type { Team, Workspace } from "../../../lib/types";
import {
  dsTaskProgressLabel,
  dsTaskRow,
  dsTaskRowsForBlock,
  expandDsTasksMention,
  groupDsTaskRows,
  parseDsTasksMention,
  resolveDsTasksPlan,
  selectDsTasks,
  type DsTasksScope,
} from "./dsTasksModel";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function task(id: string, key: string, status: Task["status"], extra: Partial<Task> = {}): Task {
  return { ...FIXTURE_TASK, id, key, name: `Task ${key}`, status, milestones: [], ...extra };
}

function plan(id: string, tasks: Task[], extra: Partial<Plan> = {}): Plan {
  return {
    id,
    title: `Plan ${id}`,
    sections: [],
    status: "active",
    owner: { tab_id: null, session_id: null, title: null },
    project_path: "/repo",
    created_at: 1,
    updated_at: 1,
    milestone_kinds: [],
    loops: [],
    elements: [],
    tasks,
    canvas: { viewport: { x: 0, y: 0, zoom: 1 } },
    owner_history: [],
    ...extra,
  } as unknown as Plan;
}

const TASKS = [
  task("task_done", "DS-1", "done"),
  task("task_review", "DS-10", "in_review"),
  task("task_pending", "DS-2", "pending"),
  task("task_paused", "DS-3", "paused"),
  task("task_progress", "DS-4", "in_progress"),
  task("task_cancel", "DS-5", "cancelled"),
  task("task_review2", "DS-9", "in_review"),
];

const TEAM_PLAN = plan("pl_team", TASKS, { team_id: "team_1", updated_at: 5 });
const WORKSPACE: Workspace = { id: "ws_1", tabs: [] } as unknown as Workspace;
const TEAM: Team = { id: "team_ws", lifecycle: "active", created_at: 1, workspace_id: "ws_1" };
const WS_OLD = plan("pl_ws_old", [], { team_id: "team_ws", updated_at: 2 });
const WS_NEW = plan("pl_ws_new", [], { team_id: "team_ws", updated_at: 9 });
const ARCHIVED = plan("pl_archived", [], { team_id: "team_1", status: "archived", updated_at: 99 });
const PLANS = [TEAM_PLAN, WS_OLD, WS_NEW, ARCHIVED];

function scope(extra: Partial<DsTasksScope> = {}): DsTasksScope {
  return { teamId: null, workspaceId: null, teams: [TEAM], workspaces: [WORKSPACE], ...extra };
}

test("a bare mention has no arguments and spans only its token", () => {
  const mention = parseDsTasksMention("look @ds-tasks please");
  assert(mention, "mention found");
  assert(mention.start === 5 && mention.end === 14, `span ${mention.start}-${mention.end}`);
  assert(!mention.args.planId && mention.args.taskRefs.length === 0 && mention.args.groups.length === 0, "no args");
});

test("plan id, task ids, keys and combinable status groups are consumed until the first other word", () => {
  const text = "@ds-tasks pl_x task_a DS-7 in_review todo pending then more";
  const mention = parseDsTasksMention(text);
  assert(mention, "mention found");
  assert(mention.args.planId === "pl_x", "plan id");
  assert(mention.args.taskRefs.join(",") === "task_a,DS-7", "task refs");
  assert(mention.args.groups.join(",") === "in_review,todo", "groups deduplicated through aliases");
  assert(text.slice(mention.end).trim() === "then more", "remaining text untouched");
});

test("a mention glued to other text is not a mention", () => {
  assert(parseDsTasksMention("mail@ds-tasks") === null, "embedded token ignored");
  assert(parseDsTasksMention("@ds-tasksx") === null, "longer token ignored");
});

test("plan resolution prefers explicit id, then the Team plan, then the workspace's newest active plan", () => {
  assert(resolveDsTasksPlan(PLANS, "pl_archived", scope())?.id === "pl_archived", "explicit id wins, any status");
  assert(resolveDsTasksPlan(PLANS, "pl_missing", scope({ teamId: "team_1" })) === null, "unknown explicit id is not replaced");
  assert(resolveDsTasksPlan(PLANS, null, scope({ teamId: "team_1", workspaceId: "ws_1" }))?.id === "pl_team", "active Team plan");
  assert(resolveDsTasksPlan(PLANS, null, scope({ teamId: "team_none", workspaceId: "ws_1" }))?.id === "pl_ws_new", "workspace fallback");
  assert(resolveDsTasksPlan(PLANS, null, scope()) === null, "no scope, no plan");
});

test("default selection drops cancelled tasks and orders by group, then natural key", () => {
  const selection = selectDsTasks(TEAM_PLAN, { taskRefs: [], groups: [] });
  assert(selection.ok, "selected");
  assert(
    selection.tasks.map((t) => t.key).join(",") === "DS-4,DS-9,DS-10,DS-2,DS-3,DS-1",
    selection.tasks.map((t) => t.key).join(","),
  );
});

test("status groups combine and todo covers pending and paused", () => {
  const selection = selectDsTasks(TEAM_PLAN, { taskRefs: [], groups: ["todo", "in_progress"] });
  assert(selection.ok, "selected");
  assert(selection.tasks.map((t) => t.id).join(",") === "task_progress,task_pending,task_paused", "union of groups");
});

test("explicit refs resolve by id or case-folded key, union with groups, and report misses", () => {
  const selection = selectDsTasks(TEAM_PLAN, { taskRefs: ["task_cancel", "ds-1"], groups: ["in_progress"] });
  assert(selection.ok, "selected");
  assert(selection.tasks.map((t) => t.id).join(",") === "task_progress,task_done,task_cancel", "explicit cancelled kept");
  const missing = selectDsTasks(TEAM_PLAN, { taskRefs: ["task_nope", "DS-1"], groups: [] });
  assert(!missing.ok && missing.missing.join(",") === "task_nope", "missing ref reported");
});

test("expansion replaces the mention with a block the chat parses back to the same task set", () => {
  const result = expandDsTasksMention("check @ds-tasks in_review now", PLANS, scope({ teamId: "team_1" }));
  assert(result.kind === "expanded", "expanded");
  const parsed = parseDsTasksMessage(result.text);
  assert(parsed, "block parses");
  assert(parsed.before === "check" && parsed.after === "now", "surrounding text kept");
  assert(parsed.block.planId === "pl_team", "plan id carried");
  assert(parsed.block.filter.join(",") === "in_review", "filter carried");
  assert(parsed.block.items.map((i) => `${i.id}:${i.status}`).join(",") === "task_review2:in_review,task_review:in_review", "items");
  assert(!result.text.includes("@ds-tasks"), "mention token not delivered");
});

test("expansion leaves plain text alone and reports unresolvable plans or tasks", () => {
  assert(expandDsTasksMention("hello", PLANS, scope()).kind === "none", "no mention");
  assert(expandDsTasksMention("@ds-tasks", PLANS, scope()).kind === "error", "no plan in scope");
  assert(expandDsTasksMention("@ds-tasks task_nope", PLANS, scope({ teamId: "team_1" })).kind === "error", "missing task");
});

test("live rows follow the plan and fall back to the message for tasks the plan no longer has", () => {
  const block = {
    planId: "pl_team",
    planTitle: "Plan",
    filter: [],
    items: [
      { id: "task_review", key: "DS-10", status: "pending", name: "Old name" },
      { id: "task_gone", key: "DS-99", status: "in_progress", name: "Gone" },
    ],
  };
  const rows = dsTaskRowsForBlock(block, TEAM_PLAN);
  assert(rows[0].status === "in_review" && !rows[0].detached, "live status wins");
  assert(rows[1].status === "in_progress" && rows[1].detached, "detached row keeps message status");
  const offline = dsTaskRowsForBlock(block, undefined);
  assert(offline.every((row) => row.detached), "no plan data, all rows detached");
});

test("rows carry the latest executor, its model and stride/criteria counts", () => {
  const row = dsTaskRow(task("task_x", "DS-8", "in_progress", {
    model: "plan-model",
    acceptance_criteria: ["a", "b"],
    milestones: [FIXTURE_MILESTONE],
    execution_attempts: [
      { tab_title: "Old", model: "old-model", assigned_at: 1 },
      { tab_title: "Fermi", session_model: "session-model", model: "assigned-model", assigned_at: 2 },
    ],
  }));
  assert(row.executor === "Fermi", "latest executor");
  assert(row.model === "session-model", "session model preferred");
  assert(row.strides === 1 && row.criteria === 2, "counts");
  assert(dsTaskProgressLabel(row) === "1 stride, 2 criteria", dsTaskProgressLabel(row));
  assert(dsTaskRow(task("task_y", "DS-9", "pending", { model: "plan-model" })).model === "plan-model", "task model fallback");
});

test("groups follow in progress, in review, blocked, to do, done, cancelled; done collapses", () => {
  const rows = TASKS.concat(task("task_blocked", "DS-6", "blocked")).map(dsTaskRow);
  const groups = groupDsTaskRows(rows);
  assert(groups.map((g) => g.key).join(",") === "in_progress,in_review,blocked,todo,done,cancelled", groups.map((g) => g.key).join(","));
  assert(groups.find((g) => g.key === "in_review")?.rows.map((r) => r.key).join(",") === "DS-9,DS-10", "natural key order");
  assert(groups.filter((g) => g.collapsedByDefault).map((g) => g.key).join(",") === "done,cancelled", "collapsed groups");
  assert(groupDsTaskRows([]).length === 0, "no rows, no groups");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    failed++;
    console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
  }
}
if (failed > 0) process.exit(1);
console.log(`\n✓ all ${tests.length} dsTasksModel tests green`);
