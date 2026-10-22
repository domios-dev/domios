import assert from "node:assert/strict";
import { curatorPresentation, collapseChecklistEvents } from "./missionControlEventPresentation";
import { eventLogRows, eventKindLabel } from "./missionControlBoardModel";
import type { PlanEvent } from "./missionControlState";

const record = { number: 7, outcome: "mode_changed", move_line: "opaque move", summary: "fallback", next_delay_secs: 7200,
  retro: ["opaque retro"], learn: ["opaque learning"], next_checks: ["opaque check"], asked: "orchestrator", ready: "opaque readiness" };
const stored = JSON.stringify(record);
const presented = curatorPresentation(stored)!;
assert(presented.preview.includes(record.move_line));
assert(!presented.preview.includes(record.retro[0]));
for (const value of [...record.retro, ...record.learn, ...record.next_checks, record.asked, record.ready]) assert(presented.body.includes(value));
assert(!presented.body.includes('"next_delay_secs"'));
assert.equal(JSON.parse(stored).next_checks[0], record.next_checks[0]);
for (const invalid of ["{", "null", "[]", '{}']) assert.equal(curatorPresentation(invalid), null);
const entry = (task: string, text = "opaque checklist"): PlanEvent => ({ ts: 100, last_ts: 100, count: 1,
  kind: "tab_todo_linked", author: task, author_tab_id: task, task_id: task, task_name: task, text });
const inputs = [entry("task_a"), entry("task_b"), entry("task_b"), entry("task_c", "different"), entry("task_a")];
const groups = collapseChecklistEvents(inputs);
assert.deepEqual(groups.map(({ event }) => event.count), [3, 1, 1]);
assert.deepEqual(groups[0].tasks.map(({ id }) => id), ["task_a", "task_b"]);
assert.equal(inputs[0].count, 1);
const rows = eventLogRows({ entries: inputs, recorded_events: inputs.length, truncated: false }).filter(row => row.kind === "event");
assert.equal(rows.length, 3);
assert.deepEqual(rows[0].event.linkedTasks.map(({ id }) => id), ["task_a", "task_b"]);
assert.equal(eventKindLabel("opaque_internal_kind").includes("_"), false);
const roundRows = eventLogRows({ entries: [{ ...entry("task_a"), kind: "curator_round", text: stored }], recorded_events: 1, truncated: false });
const roundRow = roundRows.find(row => row.kind === "event");
assert(roundRow?.kind === "event" && roundRow.event.expandable);
assert.equal(roundRow.event.preview, presented.preview);
console.log("5 event presentation behavior cases passed");
