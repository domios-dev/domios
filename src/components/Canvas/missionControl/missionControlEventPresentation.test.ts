import { formatDuration } from "../../../lib/timeLabels";
import assert from "node:assert/strict";
import { curatorPresentation } from "./missionControlEventPresentation";
import { eventLogRows, eventKindLabel, eventPreview, EVENT_PREVIEW_MAX } from "./missionControlBoardModel";
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
const tasks = [{ id: "task_a", name: "opaque a" }, { id: "task_b", name: "opaque b" }];
const inputs = [{ ...entry("task_a"), count: 3, linked_tasks: tasks }, entry("task_c", "different"), entry("task_a")];
const rows = eventLogRows({ entries: inputs, recorded_events: 5, truncated: false }).filter(row => row.kind === "event");
assert.equal(rows.length, 3);
assert.deepEqual(rows[0].event.linkedTasks, tasks);
assert.equal(inputs[0].count, 3);
assert.equal(eventKindLabel("opaque_internal_kind").includes("_"), false);
const roundRows = eventLogRows({ entries: [{ ...entry("task_a"), kind: "curator_round", text: stored }], recorded_events: 1, truncated: false });
const roundRow = roundRows.find(row => row.kind === "event");
assert(roundRow?.kind === "event" && roundRow.event.expandable);
assert.equal(roundRow.event.preview, eventPreview(presented.preview));
for (const secs of [60, 7 * 24 * 3600]) {
  const presentation = curatorPresentation(JSON.stringify({ ...record, next_delay_secs: secs }))!;
  assert(presentation.preview.includes(formatDuration(secs * 1000)));
}
const longMove = "opaque long movement ".repeat(30);
const longRecord = JSON.stringify({ ...record, move_line: longMove });
const longRows = eventLogRows({ entries: [{ ...entry("task_a"), kind: "curator_round", text: longRecord }], recorded_events: 1, truncated: false });
const longRow = longRows.find(row => row.kind === "event");
assert(longRow?.kind === "event");
assert(longRow.event.preview.length <= EVENT_PREVIEW_MAX + 1);
assert(!longRow.event.preview.includes("\n"));
assert(longRow.event.body.includes(longMove));
assert(longRow.event.expandable);
const unrecorded = curatorPresentation(JSON.stringify({ ...record, asked: "" }))!;
assert(!unrecorded.body.includes("ASKED:"));
assert(curatorPresentation(JSON.stringify({ ...record, asked: "none" }))!.body.includes("ASKED:"));
console.log("8 event presentation behavior cases passed");

const nullRun = { number: 5, started_at: 1720000000, ended_at: 1720000000, run_id: null, tab_id: null,
  outcome: "fallback", move_line: "", summary: "provider_unavailable", reason: "provider_unavailable",
  retro: [], learn: [], next_checks: [], asked: "", ready: "", next_delay_secs: 7200 };
const skipped = curatorPresentation(JSON.stringify(nullRun))!;
assert.equal(skipped.preview, `round 5 skipped: provider unavailable · next in ${formatDuration(7200000)}`);
assert(skipped.body.includes("skipped: provider unavailable"));
assert.equal(eventKindLabel("curator_round"), "Curator");
for (const outcome of ["fallback", "failed", "completed"]) {
  const empty = curatorPresentation(JSON.stringify({ ...nullRun, outcome, summary: "", reason: "" }))!;
  assert(!empty.preview.includes(" ·  · "));
  assert(empty.preview.startsWith("round 5 skipped"));
}
assert(curatorPresentation(JSON.stringify({ ...nullRun, reason: "", summary: "budget_exhausted" }))!.preview.includes("budget exhausted"));
