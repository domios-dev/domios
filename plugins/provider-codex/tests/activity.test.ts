import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import plugin from "../src/plugin";
import type { ActivityMessage } from "../src/activity";

interface Parser {
  parseSessionDelta(chunk: string): { messages: ActivityMessage[] };
}

const context = { module: { exports: {} as { default: Parser } }, exports: {}, console };
new vm.Script(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
const parsers = [plugin as unknown as Parser, context.module.exports.default];
const timestamp = "2026-01-02T03:04:05Z";
const command = "runner opaque-input";
const output = "opaque-final-output";
const completed = (item: Record<string, unknown>) => JSON.stringify({
  type: "event_msg", timestamp, payload: { type: "item_completed", item },
});
const execution = {
  type: "CommandExecution", id: "exec-opaque-a", command, status: "completed", exit_code: 0,
  aggregated_output: output,
};
const activity = {
  type: "SubAgentActivity", id: "activity-opaque-b", kind: "completed",
  agent_thread_id: "thread-opaque", agent_path: "/root/opaque-worker",
};

for (const parser of parsers) {
  const parse = (lines: string[]) => JSON.parse(JSON.stringify(parser.parseSessionDelta(lines.join("\n")))).messages as ActivityMessage[];
  const messages = parse([completed(execution), completed(activity), completed(execution)]);
  assert.equal(messages.length, 2);
  assert.equal(messages[0].uuid, `activity-${execution.id}`);
  assert.equal(messages[0].timestamp, timestamp);
  assert.equal(messages[0].blocks[0].data.id, execution.id);
  assert.equal(messages[0].blocks[0].data.tool_detail, command);
  assert.equal(messages[0].blocks[1].data.tool_use_id, execution.id);
  assert.equal(messages[0].blocks[1].data.content, output);
  assert.equal(messages[0].blocks[1].data.is_error, false);
  assert.equal(messages[1].blocks[0].data.tool_kind, "task");
  assert.deepEqual(messages[1].blocks[0].data.input, {
    kind: activity.kind, agent_path: activity.agent_path, agent_thread_id: activity.agent_thread_id,
  });
  assert.deepEqual(parse([completed(execution)]), [messages[0]]);
  assert.equal(parse([completed(execution), completed({ ...execution, id: "exec-opaque-c" })]).length, 2);

  const failed = parse([completed({ ...execution, status: "failed", exit_code: 7, aggregated_output: "", stdout: "opaque-out", stderr: "opaque-err" })])[0];
  assert.equal(failed.blocks[0].data.tool_status, "error");
  assert.equal(failed.blocks[1].data.is_error, true);
  assert.equal(failed.blocks[1].data.content, "opaque-out\nopaque-err");
  assert.equal(parse([completed({ ...execution, exit_code: 2 })])[0].blocks[1].data.is_error, true);
  assert.equal(parse([completed({ ...execution, aggregated_output: "" })])[0].blocks[0].data.tool_status, "success");

  const ignored = [
    { ...execution, id: "" }, { ...execution, command: "" }, { ...execution, status: "in_progress" },
    { ...activity, agent_path: "" }, { type: "AgentMessage", id: "opaque", text: "opaque-text" },
    { type: "Reasoning", id: "opaque" },
  ];
  assert.deepEqual(parse(ignored.map(completed)), []);

  const wrapper = JSON.stringify({ type: "response_item", payload: {
    type: "custom_tool_call", name: "exec", call_id: "outer-opaque",
    input: `text(await tools.exec_command({cmd: ${JSON.stringify(command)}}));`,
  } });
  const result = JSON.stringify({ type: "response_item", payload: {
    type: "custom_tool_call_output", call_id: "outer-opaque", output: "opaque-running-handle",
  } });
  const batch = parse([wrapper, result, completed(execution)]);
  assert.equal(batch.length, 3);
  assert.equal(batch[0].blocks[0].data.id, "outer-opaque");
  assert.equal(batch[1].blocks[0].data.tool_use_id, "outer-opaque");
  assert.equal(batch[2].blocks[0].data.id, execution.id);
  assert.equal(batch[2].blocks[1].data.content, output);
}

console.log("Codex structured tool activity source/bundle fixtures passed");
