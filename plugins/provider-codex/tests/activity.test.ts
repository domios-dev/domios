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
  duration_ms: 1250, aggregated_output: output,
};
const realShapeExecution = {
  type: "CommandExecution", id: "exec-real-duration", command, status: "completed", exit_code: 0,
  duration: { secs: 0, nanos: 226414700 }, aggregated_output: output,
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
  assert.equal(messages[0].blocks[0].data.tool_exit_code, 0);
  assert.equal(messages[0].blocks[0].data.tool_duration_ms, 1250);
  assert.equal(messages[0].blocks[1].data.tool_use_id, execution.id);
  assert.equal(messages[0].blocks[1].data.tool_exit_code, 0);
  assert.equal(messages[0].blocks[1].data.tool_duration_ms, 1250);
  assert.equal(messages[0].blocks[1].data.content, output);
  assert.equal(messages[0].blocks[1].data.is_error, false);
  const realShape = parse([completed(realShapeExecution)])[0];
  assert.equal(realShape.blocks[0].data.tool_duration_ms, 226);
  assert.equal(realShape.blocks[1].data.tool_duration_ms, 226);
  const malformedDuration = parse([completed({
    ...execution, id: "exec-malformed-duration", duration_ms: undefined,
    duration: { secs: 0, nanos: "226414700" },
  })])[0];
  assert.equal(malformedDuration.blocks[0].data.tool_duration_ms, undefined);
  assert.equal(malformedDuration.blocks[1].data.tool_duration_ms, undefined);
  assert.equal(messages[1].uuid, `agent-${activity.id}`);
  assert.deepEqual(messages[1].blocks, [{ kind: "tool_use", data: {
    name: "agent_activity", input: { agent_id: activity.agent_thread_id, agent_path: activity.agent_path, agent_state: "completed" },
    tool_kind: "agent", tool_summary: activity.agent_path, tool_status: "success",
  } }], "a sub-agent crosses the boundary as the neutral agent capability");
  assert.deepEqual(parse([completed(execution)]), [messages[0]]);
  assert.equal(parse([completed(execution), completed({ ...execution, id: "exec-opaque-c" })]).length, 2);

  const failed = parse([completed({ ...execution, status: "failed", exit_code: 7, aggregated_output: "", stdout: "opaque-out", stderr: "opaque-err" })])[0];
  assert.equal(failed.blocks[0].data.tool_status, "error");
  assert.equal(failed.blocks[1].data.is_error, true);
  assert.equal(failed.blocks[1].data.content, "opaque-out\nopaque-err");
  assert.equal(parse([completed({ ...execution, exit_code: 2 })])[0].blocks[1].data.is_error, true);
  assert.equal(parse([completed({ ...execution, exit_code: "2" })])[0].blocks[1].data.tool_exit_code, undefined);
  assert.equal(parse([completed({ ...execution, duration_ms: "slow" })])[0].blocks[1].data.tool_duration_ms, undefined);
  assert.equal(parse([completed({ ...execution, aggregated_output: "" })])[0].blocks[0].data.tool_status, "success");

  const argv = ["C:\\Opaque Tools\\pwsh.exe", "-Command", "runner --input 'opaque value'; runner opaque-tail"];
  const argvExecution = { ...execution, id: "exec-opaque-argv", command: argv };
  const argvMessage = parse([completed(argvExecution)])[0];
  assert.equal(argvMessage.uuid, `activity-${argvExecution.id}`);
  assert.equal(argvMessage.blocks[0].data.id, argvExecution.id);
  const argvInput = argvMessage.blocks[0].data.input as { command: string; argv: string[] };
  assert.deepEqual(argvInput.argv, argv);
  assert.equal(argvInput.command, `${JSON.stringify(argv[0])} ${argv[1]} ${JSON.stringify(argv[2])}`);
  assert.equal(argvMessage.blocks[0].data.tool_detail, argvInput.command);
  assert.equal(argvMessage.blocks[1].data.content, output);
  assert.equal(argvMessage.blocks[1].data.tool_use_id, argvExecution.id);
  assert.equal(argvMessage.blocks[1].data.is_error, false);
  assert.deepEqual(parse([completed(argvExecution)]), [argvMessage]);

  const unixArgv = ["/bin/sh", "-c", "runner opaque-input", ""];
  const unixInput = parse([completed({ ...execution, command: unixArgv })])[0].blocks[0].data.input as { command: string; argv: string[] };
  assert.deepEqual(unixInput.argv, unixArgv);
  assert.equal(unixInput.command, '/bin/sh -c "runner opaque-input" ""');

  const ignored = [
    { ...execution, id: "" }, { ...execution, command: "" }, { ...execution, status: "in_progress" },
    { ...execution, command: [] }, { ...execution, command: ["", "opaque"] },
    { ...execution, command: ["runner", 7] },
    { ...activity, agent_path: "", agent_thread_id: "" }, { type: "AgentMessage", id: "opaque", text: "opaque-text" },
    { type: "Reasoning", id: "opaque" },
    { type: "Reasoning", id: "rs-empty", summary_text: [], raw_content: ["opaque-raw"] },
    { type: "Reasoning", id: "rs-malformed", summary_text: [7, null, "  "] },
    { type: "Reasoning", id: "", summary_text: ["opaque-idless"] },
    { type: "CollabAgentToolCall", id: "call-wait", tool: "wait", status: "completed", sender_thread_id: "thread-root",
      receiver_thread_ids: [], receiver_agents: [], agents_states: {} },
  ];
  assert.deepEqual(parse(ignored.map(completed)), []);

  const withTurn = (line: string, turnId: string) => line.replace('"item_completed",', `"item_completed","turn_id":"${turnId}",`);
  const responseReasoning = (id: string, summary: unknown[]) => JSON.stringify({ type: "response_item", timestamp, payload: {
    type: "reasoning", id, summary, encrypted_content: "opaque-encrypted", internal_chat_message_metadata_passthrough: { turn_id: "turn-r" },
  } });
  const reasoning = parse([
    withTurn(completed({ type: "Reasoning", id: "rs-a", summary_text: ["**Inspect sources**\n\nReading files", "", "Run tests"], raw_content: ["opaque-raw"] }), "turn-r"),
    responseReasoning("rs-a", [{ type: "summary_text", text: "**Inspect sources**\n\nReading files" }]),
    responseReasoning("rs-b", [{ type: "summary_text", text: "Response summary" }, { type: "reasoning_text", text: "opaque-other" }]),
    responseReasoning("rs-c", []),
  ]);
  assert.deepEqual(reasoning.map((message) => message.uuid), ["reasoning-rs-a", "reasoning-rs-b"], "one reasoning id renders once and empty summaries are dropped");
  assert.deepEqual(reasoning[0].blocks, [
    { kind: "thinking", data: { text: "**Inspect sources**\n\nReading files" } },
    { kind: "thinking", data: { text: "Run tests" } },
  ]);
  assert.deepEqual(reasoning[1].blocks, [{ kind: "thinking", data: { text: "Response summary" } }]);
  assert.ok(!JSON.stringify(reasoning).includes("opaque"), "encrypted, raw, and non-summary reasoning never crosses the boundary");

  const collab = parse([withTurn(completed({
    type: "CollabAgentToolCall", id: "call-spawn", tool: "spawn_agent", status: "completed", sender_thread_id: "thread-root",
    receiver_thread_ids: ["thread-w", "thread-w", 7, "thread-x"], receiver_agents: [{ thread_id: "thread-w", agent_path: "/root/worker" }],
    agents_states: { "thread-w": { status: "running" }, "thread-x": { status: "mystery" } },
  }), "turn-c")]);
  assert.deepEqual(collab[0].blocks.map((block) => block.data.input), [
    { agent_id: "thread-w", agent_path: "/root/worker", agent_state: "running" },
    { agent_id: "thread-x", agent_state: "unknown" },
  ], "collaboration receivers keep proven paths and unknown states, never a guessed sender edge");
  assert.deepEqual(collab[0].blocks.map((block) => [block.kind, block.data.tool_kind, block.data.tool_flow_id]), [
    ["tool_use", "agent", "turn-c"], ["tool_use", "agent", "turn-c"],
  ]);

  const subAgentEvent = JSON.stringify({ type: "event_msg", timestamp, payload: {
    type: "sub_agent_activity", event_id: "call-evt", occurred_at_ms: 1, agent_thread_id: "thread-w", agent_path: "/root/worker", kind: "interacted",
  } });
  const subAgent = parse([subAgentEvent, completed({ type: "SubAgentActivity", id: "call-evt", kind: "interacted", agent_thread_id: "thread-w", agent_path: "/root/worker" })]);
  assert.equal(subAgent.length, 1, "the event and its completion item are one record");
  assert.deepEqual(subAgent[0].blocks[0].data.input, { agent_id: "thread-w", agent_path: "/root/worker", agent_state: "unknown" });

  const agentMessage = (id: string, author: unknown, recipient: unknown) => JSON.stringify({ type: "response_item", timestamp, payload: {
    type: "agent_message", id, author, recipient,
    content: [{ type: "input_text", text: "opaque-task" }, { type: "encrypted_content", encrypted_content: "opaque-encrypted" }],
    internal_chat_message_metadata_passthrough: { turn_id: "turn-m" },
  } });
  const edges = parse([
    agentMessage("amsg-1", "/root", "/root/worker"), agentMessage("amsg-2", "/root", "/root"),
    agentMessage("amsg-3", "", "/root/worker"), agentMessage("amsg-4", "/root", 7),
  ]);
  assert.deepEqual(edges.map((message) => message.blocks), [[{ kind: "tool_use", data: {
    name: "agent_activity", input: { message_from: "/root", message_to: "/root/worker" },
    tool_kind: "agent", tool_summary: "/root → /root/worker", tool_status: "success", tool_flow_id: "turn-m",
  } }]], "only a distinct author and recipient form a message edge");
  assert.ok(!JSON.stringify(edges).includes("opaque"), "inter-agent message bodies never cross the boundary");
  const quietWarn = console.warn;
  try {
    console.warn = () => {};
    assert.deepEqual(parse([JSON.stringify({ type: "inter_agent_communication_metadata", timestamp, payload: { trigger_turn: true } })]), [],
      "communication metadata without identities invents no edge");
  } finally {
    console.warn = quietWarn;
  }

  const wrapper = JSON.stringify({ type: "response_item", payload: {
    type: "custom_tool_call", name: "exec", call_id: "outer-opaque",
    input: `text(await tools.exec_command({cmd: ${JSON.stringify(command)}}));`,
    internal_chat_message_metadata_passthrough: { turn_id: "turn-wrapper" },
  } });
  const result = JSON.stringify({ type: "response_item", payload: {
    type: "custom_tool_call_output", call_id: "outer-opaque", output: "opaque-running-handle",
  } });
  const wrappedExecution = { ...execution, id: "exec-wrapper-pair", command: ["C:\\Opaque Tools\\pwsh.exe", "-Command", command] };
  const batch = parse([wrapper, completed(wrappedExecution).replace('"item_completed",', '"item_completed","turn_id":"turn-wrapper",'), result]);
  assert.equal(batch.length, 2);
  assert.equal(batch[0].blocks[0].data.id, "outer-opaque");
  assert.equal(batch[0].blocks[0].data.tool_summary, command);
  assert.equal(batch[1].blocks[0].data.tool_use_id, "outer-opaque");
  assert.equal(batch[1].blocks[0].data.content, "opaque-running-handle");
  assert.ok(String(batch[1].blocks[0].data.tool_wrapper_detail).includes("-Command"));
  assert.equal(messages[0].blocks[0].data.tool_flow_id, undefined, "activity without a source turn keeps its flow unknown");

  const turn = (type: "task_started" | "task_complete", turnId?: string) => JSON.stringify({
    type: "event_msg", timestamp, payload: { type, ...(turnId ? { turn_id: turnId } : {}) },
  });
  const custom = (callId: string, input: string) => JSON.stringify({
    type: "response_item", timestamp, payload: { type: "custom_tool_call", name: "exec", call_id: callId, input },
  });
  const waitAgentArgs = { ids: ["agent-opaque"], timeout_ms: 30000 };
  const commandScript = `const r = await tools.exec_command({cmd:"npm test -- --run"}); text(r.output);`;
  const pollScript = `const r = await tools.write_stdin({session_id:7,chars:"",yield_time_ms:1000}); text(r.output);`;
  const flow = parse([
    turn("task_started", "turn-a"),
    custom("call-a1", commandScript),
    custom("call-a2", pollScript),
    JSON.stringify({ type: "response_item", timestamp, payload: {
      type: "function_call", name: "wait_agent", call_id: "call-a3", arguments: JSON.stringify(waitAgentArgs),
    } }),
    completed({ ...execution, id: "exec-turn-a" }).replace('"item_completed",', '"item_completed","turn_id":"turn-a",'),
    JSON.stringify({ type: "response_item", timestamp, payload: {
      type: "custom_tool_call_output", call_id: "call-a1", output: "opaque-command-output",
    } }),
    turn("task_complete", "turn-a"),
    turn("task_started", "turn-b"),
    custom("call-b1", `const r = await tools.view_image({path:"/repo/shot.png"}); image(r.image_url);`),
  ]);
  assert.deepEqual(flow.map((message) => message.blocks[0].kind), [
    "tool_flow", "tool_use", "tool_use", "tool_use", "tool_use", "tool_result", "tool_flow", "tool_flow", "tool_use",
  ]);
  assert.deepEqual(flow[0].blocks[0].data, { state: "start", flow_id: "turn-a" });
  assert.deepEqual(flow[6].blocks[0].data, { state: "end", flow_id: "turn-a" });
  assert.deepEqual(flow[7].blocks[0].data, { state: "start", flow_id: "turn-b" });
  assert.equal(flow[1].blocks[0].data.tool_title, "npm test");
  assert.deepEqual(flow[1].blocks[0].data.input, { raw: commandScript });
  assert.equal(flow[1].blocks[0].data.tool_flow_id, undefined, "calls rely on the boundary row instead of a guessed turn");
  assert.equal(flow[1].blocks[0].data.tool_echo, undefined, "the logical call is emitted at its source position");
  assert.equal(flow[2].blocks[0].data.tool_title, "Wait for command output");
  assert.equal(flow[2].blocks[0].data.tool_summary, "Wait for command output");
  assert.equal(flow[3].blocks[0].data.name, "wait_agent");
  assert.equal(flow[3].blocks[0].data.tool_title, "wait_agent");
  assert.deepEqual(flow[3].blocks[0].data.input, waitAgentArgs);
  assert.equal(flow[4].blocks[0].data.tool_flow_id, "turn-a");
  assert.equal(flow[4].blocks[0].data.tool_title, command);
  assert.equal(flow[4].blocks[0].data.tool_echo, true, "an unmatched command completion remains visible as an echo");
  assert.equal(flow[4].blocks[1].kind, "tool_result");
  assert.equal(flow[4].blocks[1].data.tool_use_id, "exec-turn-a");
  assert.equal(flow[4].blocks[1].data.content, output, "the command completion result stays in its activity row");
  assert.equal(flow[5].blocks[0].data.content, "opaque-command-output");
  assert.equal(flow[8].blocks[0].data.tool_title, "Read shot.png");

  const listOutput = [
    { type: "input_text", text: "Script completed\nWall time 1.2 seconds\nOutput:\n" },
    { type: "input_text", text: "  accepted    true\n" },
  ];
  const paired = parse([
    custom("call-list", commandScript),
    JSON.stringify({ type: "response_item", timestamp, payload: { type: "custom_tool_call_output", call_id: "call-list", output: listOutput } }),
    JSON.stringify({ type: "response_item", timestamp, payload: {
      type: "function_call", name: "exec_command", call_id: "call-string", arguments: JSON.stringify({ cmd: "git status" }),
    } }),
    JSON.stringify({ type: "response_item", timestamp, payload: { type: "function_call_output", call_id: "call-string", output: "clean" } }),
  ]);
  assert.equal(paired[1].blocks[0].data.tool_use_id, paired[0].blocks[0].data.id, "a list-shaped exec output pairs with its call id");
  assert.equal(paired[1].blocks[0].data.content, "Script completed\nWall time 1.2 seconds\nOutput:\n  accepted    true\n");
  assert.equal(paired[3].blocks[0].data.tool_use_id, paired[2].blocks[0].data.id, "a string exec_command output pairs with its call id");
  assert.equal(paired[3].blocks[0].data.content, "clean");
  const aborted = parse([JSON.stringify({ type: "event_msg", timestamp, payload: {
    type: "turn_aborted", turn_id: "turn-z", reason: "interrupted", completed_at: 1784206857, duration_ms: 22649,
  } })]);
  assert.deepEqual(aborted[0].blocks[0].data, { state: "end", flow_id: "turn-z" }, "an aborted turn closes its flow");

  const readCommand = "Get-Content -Raw 'C:\\repo\\SKILL.md'";
  const readExecution = { ...execution, id: "exec-read", command: readCommand,
    parsed_cmd: [{ type: "read", cmd: readCommand, name: "SKILL.md", path: "C:/repo/SKILL.md" }] };
  assert.equal(parse([completed(readExecution)])[0].blocks[0].data.tool_title, "Read SKILL.md", "a parsed read names its file");
  assert.equal(parse([completed(readExecution)])[0].blocks[0].data.tool_detail, readCommand, "the exact command stays in the detail");
  const unknownParsed = { ...execution, id: "exec-unknown", parsed_cmd: [{ type: "unknown", cmd: command }] };
  assert.equal(parse([completed(unknownParsed)])[0].blocks[0].data.tool_title, command, "an unknown parse keeps the command head");
  const malformedParsed = { ...execution, id: "exec-malformed", parsed_cmd: [null, 7] };
  assert.equal(parse([completed(malformedParsed)])[0].blocks[0].data.tool_title, command, "malformed parsed_cmd falls back to the command head");
  const originalWarn = console.warn;
  try {
    console.warn = () => {};
    assert.deepEqual(
      parse(["{not json", JSON.stringify({ type: "event_msg", timestamp, payload: { type: "turn_aborted" } })]),
      [],
      "malformed lines and id-less aborts are skipped without breaking the parse",
    );
  } finally {
    console.warn = originalWarn;
  }
  assert.deepEqual(parse([turn("task_started"), turn("task_complete")]), [], "a boundary without a source turn id is dropped");
}

console.log("Codex structured tool activity source/bundle fixtures passed");
