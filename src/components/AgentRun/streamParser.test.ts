import {
  ClaudeStreamParser,
  mergeAssistantMessages,
  reconcileToolInvocations,
  selectEffectiveToolInvocations,
  type ClaudeStreamEvent,
} from "./streamParser";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function eq(actual: unknown, expected: unknown, message: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) {
    throw new Error(`${message}: expected ${e}, got ${a}`);
  }
}

function parseAll(chunks: string[]): ClaudeStreamEvent[] {
  const parser = new ClaudeStreamParser();
  const events: ClaudeStreamEvent[] = [];
  for (const chunk of chunks) {
    events.push(...parser.feed(chunk).events);
  }
  return mergeAssistantMessages(events);
}

function toolUse(
  toolUseId: string,
  name: string,
  input: unknown,
  messageId = `${toolUseId}-call`,
) {
  return JSON.stringify({
    type: "assistant",
    message: {
      id: messageId,
      content: [{ type: "tool_use", id: toolUseId, name, input }],
    },
  });
}

function toolResult(
  toolUseId: string,
  content: unknown,
  isError = false,
  messageId = `${toolUseId}-result`,
) {
  return JSON.stringify({
    type: "user",
    message: {
      id: messageId,
      content: [{ type: "tool_result", tool_use_id: toolUseId, content, is_error: isError }],
    },
  });
}

function states(chunks: string[]) {
  return reconcileToolInvocations(parseAll(chunks));
}

function effectiveStates(chunks: string[]) {
  return selectEffectiveToolInvocations(states(chunks));
}

{
  const invocations = states([
    toolUse("ok-1", "Bash", { command: "echo ok" }),
    toolResult("ok-1", "done"),
  ]);
  eq(
    invocations.map((item) => [item.key, item.state, item.result, item.isError, item.terminalTransitionCount]),
    [["tool:ok-1", "success", "done", false, 1]],
    "success invocation reaches one stable terminal tuple",
  );
}

{
  const invocations = states([
    toolUse("fail-1", "Bash", { command: "false" }),
    toolResult("fail-1", "command failed", true),
  ]);
  eq(invocations[0]?.state, "failure", "error result classifies as failure");
}

{
  const invocations = states([
    toolUse("timeout-1", "Bash", { command: "sleep 10" }),
    toolResult("timeout-1", "timed out after 10s", true),
  ]);
  eq(invocations[0]?.state, "timeout", "timeout result classifies as timeout");
}

{
  const invocations = states([
    toolUse("cancel-1", "Bash", { command: "tail -f" }),
    toolResult("cancel-1", "cancelled by user", true),
  ]);
  eq(invocations[0]?.state, "cancelled", "cancelled result classifies as cancelled");
}

{
  const invocations = states([
    toolUse("late-1", "Bash", { command: "echo ok" }),
    toolResult("late-1", "done"),
    toolResult("late-1", "timed out after 1s", true, "late-1-timeout-dup"),
  ]);
  eq(
    [invocations[0]?.state, invocations[0]?.result, invocations[0]?.isError, invocations[0]?.terminalTransitionCount],
    ["success", "done", false, 1],
    "late terminal event leaves full terminal tuple immutable",
  );
}

{
  const invocations = states([
    toolUse("dup-1", "Bash", { command: "echo ok" }),
    toolResult("dup-1", "done"),
    toolResult("dup-1", "done", false, "dup-1-result-replay"),
  ]);
  eq(
    [invocations[0]?.state, invocations[0]?.result, invocations[0]?.isError, invocations[0]?.terminalTransitionCount],
    ["success", "done", false, 1],
    "duplicate result does not mutate terminal tuple",
  );
}

{
  const invocations = effectiveStates([
    toolUse("exec-1", "Bash", { command: "npm test" }),
    toolUse("wait-1", "Wait", { command_id: "exec-1" }),
    toolResult("wait-1", "done"),
  ]);
  const parent = invocations.find((item) => item.toolUseId === "exec-1");
  const wait = invocations.find((item) => item.toolUseId === "wait-1");
  eq(wait?.link, {
    parentInvocationKey: "tool:exec-1",
    relationship: "wait",
  }, "wait continuation explicitly links to parent exec");
  eq(
    [parent?.state, parent?.effectiveState, parent?.effectiveResult, parent?.effectiveIsError, parent?.terminalTransitionCount],
    ["success", "success", "done", false, 1],
    "wait completion terminalizes the linked parent exactly once",
  );
}

{
  const invocations = effectiveStates([
    toolUse("exec-timeout", "Bash", { command: "sleep 20" }),
    toolUse("wait-timeout", "Wait", { command_id: "exec-timeout" }),
    toolResult("wait-timeout", "timeout after 20s", true),
  ]);
  const parent = invocations.find((item) => item.toolUseId === "exec-timeout");
  eq(
    [parent?.state, parent?.effectiveState, parent?.effectiveResult, parent?.effectiveIsError, parent?.terminalTransitionCount],
    ["timeout", "timeout", "timeout after 20s", true, 1],
    "wait timeout reconciles into parent effective timeout state",
  );
}

{
  const invocations = states([
    toolUse("exec-late", "Bash", { command: "echo done" }),
    toolUse("wait-late", "Wait", { command_id: "exec-late" }),
    toolResult("wait-late", "done"),
    toolResult("exec-late", "late direct result", false, "exec-late-direct-late"),
    toolResult("wait-late", "late wait result", true, "wait-late-duplicate"),
  ]);
  eq(
    invocations.map((item) => [item.toolUseId, item.state, item.result, item.terminalTransitionCount]),
    [
      ["exec-late", "success", "done", 1],
      ["wait-late", "success", "done", 1],
    ],
    "linked phases retain their first terminal transition after late results",
  );
}

{
  const invocations = effectiveStates([
    toolUse("orphan-wait", "Wait", { command_id: "missing-parent" }),
    toolResult("orphan-wait", "done"),
  ]);
  const wait = invocations[0];
  eq(wait?.effectiveState, "success", "wait parent missing still settles its own invocation");
  eq(wait?.link?.parentInvocationKey, "tool:missing-parent", "missing parent link is preserved");
}

{
  const invocations = states([
    toolResult("before-call", "done"),
    toolUse("before-call", "Bash", { command: "echo after" }),
  ]);
  eq(
    [invocations[0]?.state, invocations[0]?.result, invocations[0]?.name],
    ["success", "done", "Bash"],
    "out-of-order result before call still converges on one terminal invocation",
  );
}

{
  const history = [
    toolUse("restore-1", "Bash", { command: "pwd" }),
    toolResult("restore-1", "done"),
  ];
  const live = states(history);
  const restored = states([history.join("")]);
  eq(
    live.map((item) => [item.key, item.state, item.result, item.isError]),
    restored.map((item) => [item.key, item.state, item.result, item.isError]),
    "reconnect replay derives same terminal state as live stream",
  );
}

{
  const events = parseAll([
    toolUse("mixed-1", "Bash", { command: "echo hi" }, "shared-msg"),
    toolResult("mixed-1", "done", false, "shared-msg"),
  ]);
  eq(
    events.flatMap((event) =>
      event.type === "assistant" || event.type === "user"
        ? [[event.type, event.message_id, event.content.length]]
        : [],
    ),
    [
      ["assistant", "shared-msg", 1],
      ["user", "shared-msg", 1],
    ],
    "same message id across different event types does not merge incorrectly",
  );
}

{
  const chunks: string[] = [];
  for (let index = 0; index < 20; index += 1) {
    chunks.push(toolUse(`parallel-${index}`, "Bash", { command: `echo ${index}` }, `msg-call-${index}`));
  }
  for (let index = 19; index >= 0; index -= 1) {
    chunks.push(toolResult(`parallel-${index}`, `done ${index}`, false, `msg-result-${index}`));
  }
  const invocations = effectiveStates(chunks);
  const keys = invocations.map((item) => item.key);
  eq(new Set(keys).size, 20, "20 interleaved commands keep unique stable keys");
  eq(
    invocations.every((item) => item.effectiveState === "success"),
    true,
    "20 interleaved commands settle independently",
  );
}

{
  const parser = new ClaudeStreamParser();
  const partA = parser.feed(toolUse("split-1", "Bash", { command: "{json}" }).slice(0, 48));
  const partB = parser.feed(toolUse("split-1", "Bash", { command: "{json}" }).slice(48));
  assert(partA.events.length === 0, "split chunk should wait for full object");
  assert(partB.events.length === 1, "split chunk should parse once complete");
}

console.log("ok - AgentRun stream parser lifecycle tests");
