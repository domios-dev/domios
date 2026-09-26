import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import plugin from "../src/plugin";
import type { ActivityMessage } from "../src/activity";

interface Parser {
  parseSessionDelta(chunk: string, context?: { session_key: string; from_offset: number }): { messages: ActivityMessage[] };
}

const context = { module: { exports: {} as { default: Parser } }, exports: {}, console };
new vm.Script(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
const parsers = [plugin as unknown as Parser, context.module.exports.default];
const fixture = fs.readFileSync(new URL("./fixtures/codex-exec-wrapper.jsonl", import.meta.url), "utf8").trim().split(/\r?\n/);
const fixtureRecords = fixture.map((line) => JSON.parse(line) as { payload: Record<string, any> });
const sourceCall = fixtureRecords.find((entry) => entry.payload.type === "custom_tool_call")!;
const sourceExecution = fixtureRecords.find((entry) => entry.payload.type === "item_completed")!;
const sourceOutput = fixtureRecords.find((entry) => entry.payload.type === "custom_tool_call_output")!;
const sourceCallId = String(sourceCall.payload.call_id);
const sourceTurnId = String(sourceExecution.payload.turn_id);

function identify(line: string, callId: string, turnId: string, executionId?: string): string {
  const entry = JSON.parse(line) as { payload: Record<string, any> };
  const payload = entry.payload;
  if (payload.type === "custom_tool_call" || payload.type === "custom_tool_call_output") payload.call_id = callId;
  if (payload.type === "custom_tool_call") payload.internal_chat_message_metadata_passthrough = { turn_id: turnId };
  if (payload.type === "item_completed") {
    payload.turn_id = turnId;
    if (executionId) payload.item.id = executionId;
  }
  return JSON.stringify(entry);
}

function recordsFor(callId: string, turnId = sourceTurnId, suffix = callId): string[] {
  return [
    identify(JSON.stringify(sourceCall), callId, turnId),
    identify(JSON.stringify(sourceExecution), callId, turnId, `exec-${suffix}`),
    identify(JSON.stringify(sourceOutput), callId, turnId),
  ];
}

function parse(
  parser: Parser,
  lines: string[],
  sessionKey: string,
  fromOffset: number,
): ActivityMessage[] {
  return JSON.parse(JSON.stringify(parser.parseSessionDelta(lines.join("\n"), {
    session_key: sessionKey,
    from_offset: fromOffset,
  }))).messages as ActivityMessage[];
}

function parseIncrementally(parser: Parser, lines: string[], sessionKey: string): ActivityMessage[] {
  const messages: ActivityMessage[] = [];
  let offset = 0;
  for (const line of lines) {
    messages.push(...parse(parser, [line], sessionKey, offset));
    offset += Buffer.byteLength(line + "\n", "utf8");
  }
  return messages;
}

function blocks(messages: ActivityMessage[], kind: string): Array<{ data: Record<string, any> }> {
  return messages.flatMap((message) => message.blocks.filter((block) => block.kind === kind));
}

function comparable(messages: ActivityMessage[]): unknown[] {
  return JSON.parse(JSON.stringify(messages)).map((message: Record<string, unknown>) => {
    const { recordIndex: _recordIndex, ...row } = message;
    return row;
  });
}

const turnAborted = (turnId: string) => JSON.stringify({
  type: "event_msg", payload: { type: "turn_aborted", turn_id: turnId, reason: "interrupted" },
});

function commandCompletion(
  id: string,
  command: string,
  turnId: string,
  status: "completed" | "failed",
  exitCode: number,
  durationMs: number,
): string {
  return JSON.stringify({
    type: "event_msg",
    payload: {
      type: "item_completed",
      turn_id: turnId,
      item: {
        type: "CommandExecution", id,
        command: ["C:\\Tools\\pwsh.exe", "-Command", command],
        status, exit_code: exitCode, duration_ms: durationMs,
      },
    },
  });
}

for (const [index, parser] of parsers.entries()) {
  const prefix = `transcript-dedupe-${index}`;

  const equivalenceSession = `${prefix}-equivalent`;
  const live = parseIncrementally(parser, fixture, equivalenceSession);
  parse(parser, [identify(JSON.stringify(sourceCall), "reparse-pending", sourceTurnId)], equivalenceSession, 0);
  const restored = parse(parser, fixture, equivalenceSession, 0);
  assert.deepEqual(comparable(live), comparable(restored), "full parsing and incremental parsing produce the same rows");
  assert.equal(blocks(restored, "tool_use").length, 1, "the real fixture produces one logical tool row");
  assert.equal(blocks(restored, "tool_result").length, 1, "the real fixture result stays attached by call id");
  assert.equal(blocks(restored, "tool_use")[0].data.id, sourceCallId);
  assert.equal(blocks(restored, "tool_use")[0].data.tool_summary, "codeterm agent startup-ack --help");
  assert.equal(blocks(restored, "tool_result")[0].data.tool_use_id, sourceCallId);
  assert.ok(String(blocks(restored, "tool_result")[0].data.tool_wrapper_detail).includes("-Command"));

  const callOnly = parse(parser, [identify(JSON.stringify(sourceCall), "call-aborted", sourceTurnId)], `${prefix}-aborted`, 0);
  assert.equal(blocks(callOnly, "tool_use").length, 1, "a call row appears before its output exists");
  assert.equal(blocks(callOnly, "tool_result").length, 0, "a missing output creates no orphan result");
  parse(parser, [turnAborted(sourceTurnId)], `${prefix}-aborted`, 1);
  const afterAbort = parse(parser, [identify(JSON.stringify(sourceExecution), "call-aborted", sourceTurnId)], `${prefix}-aborted`, 2);
  assert.equal(blocks(afterAbort, "tool_use")[0].data.tool_echo, true, "an aborted call cannot consume a later unmatched wrapper");

  const wrapperFirst = parse(parser, [
    identify(JSON.stringify(sourceExecution), "wrapper-first", sourceTurnId),
    identify(JSON.stringify(sourceCall), "wrapper-first", sourceTurnId),
    identify(JSON.stringify(sourceOutput), "wrapper-first", sourceTurnId),
  ], `${prefix}-wrapper-first`, 0);
  const wrapperFirstEchoes = blocks(wrapperFirst, "tool_use").filter((block) => block.data.name === "command_execution");
  assert.equal(wrapperFirstEchoes.length, 1, "a wrapper seen before its call remains visible as an unpaired echo");
  assert.equal(wrapperFirstEchoes[0].data.tool_echo, true);
  assert.equal(blocks(wrapperFirst, "tool_use").filter((block) => block.data.name === "exec").length, 1);

  const duplicateCallA = identify(JSON.stringify(sourceCall), "concurrent-a", "turn-concurrent");
  const duplicateCallB = identify(JSON.stringify(sourceCall), "concurrent-b", "turn-concurrent");
  const duplicateWrapA = identify(JSON.stringify(sourceExecution), "unused", "turn-concurrent", "exec-concurrent-a");
  const duplicateWrapB = identify(JSON.stringify(sourceExecution), "unused", "turn-concurrent", "exec-concurrent-b");
  const concurrent = parse(parser, [
    duplicateCallA, duplicateCallB, duplicateWrapA, duplicateWrapB,
    identify(JSON.stringify(sourceOutput), "concurrent-a", "turn-concurrent"),
    identify(JSON.stringify(sourceOutput), "concurrent-b", "turn-concurrent"),
  ], `${prefix}-concurrent`, 0);
  const concurrentCalls = blocks(concurrent, "tool_use").filter((block) => block.data.name === "exec");
  const concurrentEchoes = blocks(concurrent, "tool_use").filter((block) => block.data.name === "command_execution");
  assert.deepEqual(concurrentCalls.map((block) => block.data.id), ["concurrent-a", "concurrent-b"]);
  assert.equal(concurrentEchoes.length, 2, "ambiguous identical concurrent wrappers are not merged into either call");
  assert.ok(concurrentEchoes.every((block) => block.data.tool_echo === true));

  const sameIdentityCall = identify(JSON.stringify(sourceCall), "session-shared-call", "turn-shared");
  const sameIdentityWrap = identify(JSON.stringify(sourceExecution), "unused", "turn-shared");
  const sameIdentityOutput = identify(JSON.stringify(sourceOutput), "session-shared-call", "turn-shared");
  const sessionA: ActivityMessage[] = [];
  const sessionB: ActivityMessage[] = [];
  sessionA.push(...parse(parser, [sameIdentityCall], `${prefix}-session-a`, 0));
  sessionB.push(...parse(parser, [sameIdentityCall], `${prefix}-session-b`, 0));
  sessionA.push(...parse(parser, [sameIdentityWrap], `${prefix}-session-a`, 1));
  sessionB.push(...parse(parser, [sameIdentityWrap], `${prefix}-session-b`, 1));
  sessionA.push(...parse(parser, [sameIdentityOutput], `${prefix}-session-a`, 2));
  sessionB.push(...parse(parser, [sameIdentityOutput], `${prefix}-session-b`, 2));
  assert.equal(blocks(sessionA, "tool_use").filter((block) => block.data.name === "exec").length, 1);
  assert.equal(blocks(sessionB, "tool_use").filter((block) => block.data.name === "exec").length, 1);
  assert.equal(blocks(sessionA, "tool_use").filter((block) => block.data.name === "command_execution").length, 0);
  assert.equal(blocks(sessionB, "tool_use").filter((block) => block.data.name === "command_execution").length, 0);
  assert.equal(blocks(sessionA, "tool_result").length, 1);
  assert.equal(blocks(sessionB, "tool_result").length, 1);

  const sequential = parse(parser, [
    ...recordsFor("repeat-a", "turn-repeat", "a"),
    ...recordsFor("repeat-b", "turn-repeat", "b"),
  ], `${prefix}-sequential`, 0);
  assert.deepEqual(
    blocks(sequential, "tool_use").map((block) => block.data.id),
    ["repeat-a", "repeat-b"],
    "two identical sequential calls remain two logical calls",
  );
  assert.deepEqual(
    blocks(sequential, "tool_result").map((block) => block.data.tool_use_id),
    ["repeat-a", "repeat-b"],
  );

  const firstCommand = "first-command";
  const secondCommand = "second-command";
  const thirdCommand = "third-command";
  const multiScript = `text(await tools.exec_command({cmd:${JSON.stringify(firstCommand)}})); text(await tools.exec_command({cmd:${JSON.stringify(secondCommand)}})); text(await tools.exec_command({cmd:${JSON.stringify(thirdCommand)}}));`;
  const multiCall = JSON.stringify({
    type: "response_item",
    payload: {
      type: "custom_tool_call", name: "exec", call_id: "multi-call", input: JSON.stringify({ raw: multiScript }),
      internal_chat_message_metadata_passthrough: { turn_id: "turn-multi" },
    },
  });
  const multiOutput = JSON.stringify({ type: "response_item", payload: {
    type: "custom_tool_call_output", call_id: "multi-call", output: "both finished",
  } });
  const multi = parse(parser, [
    multiCall,
    commandCompletion("multi-fail-third", thirdCommand, "turn-multi", "failed", 9, 17),
    commandCompletion("multi-fail-first", firstCommand, "turn-multi", "failed", 7, 11),
    commandCompletion("multi-success", secondCommand, "turn-multi", "completed", 0, 13),
    multiOutput,
  ], `${prefix}-multi`, 0);
  const multiResult = blocks(multi, "tool_result")[0].data;
  assert.equal(multiResult.tool_status, "error", "any failed nested command marks the logical result failed");
  assert.equal(multiResult.is_error, true);
  assert.equal(multiResult.tool_exit_code, 7, "the aggregate keeps the first failing exit code");
  assert.equal(multiResult.tool_duration_ms, 41, "known nested command durations are summed");
  assert.equal((multiResult.tool_command_results as unknown[]).length, 3, "each nested command keeps its result");

  for (let session = 0; session < 33; session++) {
    const call = identify(JSON.stringify(sourceCall), `bounded-${session}`, "turn-bounded");
    const emitted = parse(parser, [call], `${prefix}-bounded-${session}`, 0);
    assert.equal(blocks(emitted, "tool_use").length, 1, "bounded pending state never withholds a logical call");
  }
  const evictedWrapper = parse(parser, [
    identify(JSON.stringify(sourceExecution), "unused", "turn-bounded"),
  ], `${prefix}-bounded-0`, 1);
  assert.equal(blocks(evictedWrapper, "tool_use")[0].data.tool_echo, true, "abandoned session state is bounded and cannot hide an unmatched row");
}

console.log("Codex transcript dedupe source and bundle fixtures passed");
