import assert from "node:assert/strict";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../../components/Canvas/cards/fixtures.testdata";
import { createTasksBlockHost, type PlansSnapshot } from "./tasksBlockHost";
import { createValueFeed } from "./valueFeed";

const feed = createValueFeed<PlansSnapshot>({ loaded: true, plans: [{ ...FIXTURE_PLAN, id: "pl_local", team_id: "team_local", tasks: [{ ...FIXTURE_TASK, id: "task_local", key: "COD-1447" }] }] });
const opened: unknown[] = [];
const host = createTasksBlockHost(feed, (target) => opened.push(target));
host.openTaskKey?.("cod-1447");
assert.deepEqual(opened, [{ planId: "pl_local", taskId: "task_local", teamId: "team_local" }]);
host.openTaskKey?.("COD-9999");
assert.equal(opened.length, 1);
feed.set({ loaded: true, plans: [] });
host.openTaskKey?.("COD-1447");
assert.equal(opened.length, 1);
console.log("3 task key navigation checks passed");
