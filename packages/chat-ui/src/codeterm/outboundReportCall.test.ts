import assert from "node:assert/strict";
import { test } from "node:test";
import { reduceToolEvents, type NormalizedChatMessage, type ToolEvent } from "@codeterm/chat-core";
import { outboundReportCalls, outboundReportIds } from "./outboundReportCall";

const PAD_WRITE = "cat > \"$(domios pad path)/report.md\" <<'EOF'\n## Changes\nopaque body\nEOF\n";

function finishedCall(command: string, result: string, id = "call"): ToolEvent {
  const rows: NormalizedChatMessage[] = [
    { id, type: "tool_call", content: "", toolName: "Bash", toolKind: "command", toolId: `${id}-tool`, toolInput: { command } },
    { id: `${id}-result`, type: "tool_result", content: result, toolId: `${id}-tool` },
  ];
  const item = reduceToolEvents(rows).find((candidate) => candidate.kind === "tool");
  assert.ok(item?.kind === "tool", "the call and its result reduce to one tool event");
  return item.event;
}

const humanReceipt = (reportId: string, taskReport = '{"persisted":true,"plan_id":"pl_a","report_outcome":null,"task_id":"task_b"}') => [
  "  accepted        true",
  "  ok              true",
  `  report_id       ${reportId}`,
  "  report_outcome  (none)",
  "  state           recorded",
  "  target_tab_id   4c25e56b",
  `  task_report     ${taskReport}`,
  `CODETERM_RESOLVED_TEXT=${JSON.stringify("## Changes\nopaque body")}`,
].join("\n");

test("a pad-written report call yields its receipt, task and delivered body", () => {
  const event = finishedCall(`${PAD_WRITE}domios agent report --status in_review --sha abc1234 --from-pad report.md`, humanReceipt("r-1"));
  assert.deepEqual(outboundReportCalls(event), [{
    reportId: "r-1",
    targetTabId: "4c25e56b",
    task: "pl_a/task_b",
    status: "in_review",
    sha: "abc1234",
    message: "## Changes\nopaque body",
    taskOutcome: { persisted: true, transitionRejected: false, cause: null, reportOutcome: null },
  }]);
  assert.equal(outboundReportCalls(event), outboundReportCalls(event), "one tool event parses once");
});

test("a --json receipt and a rejected transition keep the daemon's verdict", () => {
  const json = JSON.stringify({ ok: true, report_id: "r-2", target_tab_id: "parent01", report_outcome: "transition_rejected",
    task_report: { persisted: false, plan_id: "pl_a", task_id: "task_b", transition_rejected: true, cause: "opaque cause",
      report_outcome: "transition_rejected" } });
  const [call] = outboundReportCalls(finishedCall("domios agent report --json --status done --text opaque", json));
  assert.equal(call.reportId, "r-2");
  assert.equal(call.message, "opaque");
  assert.deepEqual(call.taskOutcome, { persisted: false, transitionRejected: true, cause: "opaque cause", reportOutcome: "transition_rejected" });
});

test("only accepted report calls produce cards", () => {
  const refused = finishedCall("domios agent report --status in_review --from-pad report.md",
    "!!! REPORT NOT ACCEPTED — no task transition or delivery was recorded.\ncause: opaque");
  assert.deepEqual(outboundReportCalls(refused), []);
  const other = finishedCall("domios tab list", humanReceipt("r-3"));
  assert.deepEqual(outboundReportCalls(other), [], "another command printing a report id is not a report");
});

test("report ids are read from raw printed output", () => {
  assert.deepEqual(outboundReportIds(humanReceipt("r-4")), ["r-4"]);
  assert.deepEqual(outboundReportIds("report_id r-5 inline"), []);
  assert.deepEqual(outboundReportIds(undefined), []);
});
