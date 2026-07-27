import { coalesceWaitEvents, materializeToolEvent, reduceToolEvents } from "./toolEvents";
import type { NormalizedChatMessage } from "./types";

function assert(value: unknown, label: string): asserts value {
  if (!value) throw new Error(label);
}

function call(id: string, toolId: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return { id, type: "tool_call", content: "", toolId, toolKind: "generic", toolSummary: "run", ...patch };
}

function result(id: string, toolId: string, detail: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return { id, type: "tool_result", content: "", toolId, toolKind: "generic", toolDetail: detail, ...patch };
}

{
  const kinds = ["command", "edit", "read", "search", "task", "generic"] as const;
  for (const kind of kinds) {
    const event = materializeToolEvent(call(kind, kind, { toolKind: kind, toolSummary: kind }));
    assert(event?.kind === kind, `${kind} canonical kind materializes`);
  }
}

{
  const success = materializeToolEvent(call("embedded-success", "embedded-success", { toolResult: "done" }));
  const error = materializeToolEvent(call("embedded-error", "embedded-error", { toolResult: "failed", toolError: true }));
  assert(success?.status === "success" && success.result === "done", "embedded call result resolves success");
  assert(error?.status === "error" && error.result === "failed", "embedded call error resolves failure");
}

{
  const event = materializeToolEvent(call("completed-empty", "completed-empty", { toolError: false }));
  assert(event?.status === "success", "explicit completion resolves without fake output");
  assert(event.result === undefined, "explicit completion does not invent result text");
}

{
  const items = reduceToolEvents([
    call("call", "same"),
    result("result", "same", "ok"),
    result("result", "same", "ok"),
  ]);
  assert(items.length === 1, "duplicate result is not a second timeline item");
  assert(items[0].kind === "tool" && items[0].event.status === "success", "result resolves once");
}

{
  const items = reduceToolEvents([
    { id: "call-detail", type: "tool_call", content: "", toolId: "shape", toolSummary: "run", toolKind: "command" },
    { id: "result-content", type: "tool_result", content: "content body", toolId: "shape" },
  ]);
  assert(items[0].kind === "tool" && items[0].event.message.toolResult === "content body", "content result body attaches");
  const detailItems = reduceToolEvents([
    { id: "call-detail-2", type: "tool_call", content: "", toolId: "shape-2", toolSummary: "run", toolKind: "command" },
    { id: "result-detail", type: "tool_result", content: "content body", toolDetail: "detail body", toolId: "shape-2" },
  ]);
  assert(detailItems[0].kind === "tool" && detailItems[0].event.message.toolResult === "detail body", "detail result body attaches");
  const emptyDetailItems = reduceToolEvents([
    { id: "call-detail-3", type: "tool_call", content: "", toolId: "shape-3", toolSummary: "run", toolKind: "command" },
    { id: "result-detail-3", type: "tool_result", content: "content body", toolDetail: "", toolId: "shape-3" },
  ]);
  assert(emptyDetailItems[0].kind === "tool" && emptyDetailItems[0].event.message.toolResult === "content body", "empty detail does not mask output");
}

{
  const items = reduceToolEvents([
    call("send-call", "send-id", { toolKind: "command", toolSummary: 'codeterm send "$VAR"', toolDetail: 'codeterm send "$VAR"' }),
    result("send-output", "send-id", 'OK (sent)\nCODETERM_RESOLVED_TEXT="expanded ***REDACTED*** text"'),
  ]);
  assert(items[0].kind === "tool" && items[0].event.detail === 'codeterm send "$VAR"', "raw command detail remains source as typed");
  assert(items[0].kind === "tool" && items[0].event.result?.includes("CODETERM_RESOLVED_TEXT"), "function output attaches as event result for resolved send rendering");
}

{
  const items = reduceToolEvents([
    call("error-call", "sticky"),
    result("error-result", "sticky", "failed", { toolError: true }),
    result("success-result", "sticky", "later success"),
  ]);
  assert(items[0].kind === "tool" && items[0].event.status === "error", "error status is sticky");
}

{
  const items = reduceToolEvents([
    { id: "left", type: "tool_call", content: "left" },
    { id: "right", type: "tool_call", content: "right" },
    { id: "positional-result", type: "tool_result", content: "right result" },
  ]);
  assert(items.length === 2, "positional result resolves one neighbour");
  assert(items[0].kind === "tool" && items[0].event.status === "pending", "older neighbour stays pending");
  assert(items[1].kind === "tool" && items[1].event.message.toolResult === "right result", "latest neighbour consumes result");
}

{
  const items = reduceToolEvents([result("result", "late", "ok"), call("call", "late")]);
  assert(items.length === 1, "result before call attaches on call");
  assert(items[0].kind === "tool" && items[0].event.result === "ok", "late result resolves");
}

{
  const items = reduceToolEvents([call("pending", "one"), call("other", "two"), result("r", "two", "done")]);
  assert(items.length === 2, "pending call remains in timeline");
  assert(items[0].kind === "tool" && items[0].event.status === "pending", "pending call is not consumed");
  assert(items[1].kind === "tool" && items[1].event.result === "done", "result resolves its own call");
}

{
  const items = reduceToolEvents([
    call("internal", "internal-call", { toolName: "send_message", toolSummary: "send_message" }),
    call("send", "send-call", { toolName: "exec_command", toolSummary: 'codeterm send --pane pane "hello"' }),
  ]);
  assert(items.length === 1, "internal orchestration calls do not enter the chat timeline");
  assert(items[0].kind === "tool" && items[0].event.summary === 'codeterm send --pane pane "hello"', "real codeterm send remains visible");
}

{
  const event = materializeToolEvent({ id: "legacy", type: "tool_call", content: "{raw: value}" });
  assert(event?.kind === "generic", "legacy-only tool is generic");
  assert(event?.summary === "", "legacy raw object is not used as canonical summary");
}

{
  const canonicalWithoutContent = {
    id: "canonical-edit",
    type: "tool_call",
    toolKind: "edit",
    toolSummary: "src/main.ts",
    toolEdits: [{ path: "src/main.ts", old: "a", new: "b" }],
  } as NormalizedChatMessage;
  const items = reduceToolEvents([canonicalWithoutContent]);
  assert(items[0]?.kind === "tool" && items[0].event.kind === "edit", "canonical tool call tolerates absent content");
}

{
  const items = reduceToolEvents([
    result("r1", "buffered", "first"),
    result("r2", "buffered", "second"),
    call("call", "buffered", { toolSummary: "buffered" }),
  ]);
  assert(items.length === 1, "all buffered results attach to one call");
  assert(items[0].kind === "tool" && items[0].event.result === "first\nsecond", "buffered results merge in order");
}

{
  const callEvent = materializeToolEvent({ id: "raw", type: "tool_call", content: "raw", toolArgs: "legacy", toolName: "legacy" });
  assert(callEvent?.summary === "", "content and legacy tool fields do not become summary");
  assert(callEvent?.status === "pending", "call starts pending");
}

console.log("ok - tool event lifecycle");

{
  const wait = (id: string, name: string, patch: Partial<NormalizedChatMessage> = {}) => call(id, id, {
    toolName: name,
    toolSummary: `${name} ${id}`,
    timestamp: "1000",
    sourceSessionId: "session",
    ...patch,
  });
  const items = reduceToolEvents([
    wait("wait-1", " wait ", { timestamp: "1000", toolResult: "first result" }),
    wait("wait-2", "WAIT_FOR_RESULT", { timestamp: "2400", toolResult: "second result" }),
    call("command", "command", { toolName: "wait", toolKind: "command", toolSummary: "wait command" }),
    wait("wait-3", "tool_wait"),
    { id: "assistant", type: "assistant", content: "break" },
    wait("wait-4", "wait_for"),
    { id: "result-only", type: "tool_result", content: "wait result", toolName: "wait", toolKind: "generic" },
    wait("wait-5", "wait"),
  ]);
  assert(items.length === 7, "only adjacent qualifying wait calls coalesce");
  assert(items[0].kind === "tool" && items[0].event.waitBatch, "exact aliases create a wait batch");
  assert(items[0].kind === "tool" && items[0].event.waitBatch?.members.length === 2, "batch retains every member");
  assert(items[0].kind === "tool" && items[0].event.waitBatch?.elapsedMs === 1400, "numeric timestamps derive elapsed time");
  assert(items[1].kind === "tool" && items[1].event.kind === "command", "commands never coalesce");
  assert(items[2].kind === "tool" && items[2].event.waitBatch, "command splits adjacent waits");
  assert(items[3].kind === "message", "messages preserve order and split batches");
  assert(items[5].kind === "tool" && items[5].event.phase === "result", "result-only rows never coalesce");

  const rejected = ["await", "wait-and-run", "shell_wait"];
  for (const name of rejected) {
    const output = reduceToolEvents([wait(name, name)]);
    assert(output[0].kind === "tool" && output[0].event.kind === "generic", `${name} is not an exact wait alias`);
  }
  assert(reduceToolEvents([wait("wait_agent", "wait_agent")]).length === 0, "existing internal orchestration hiding still excludes wait_agent");
  const invalidTimes = reduceToolEvents([wait("bad-1", "wait", { timestamp: "not-a-number" }), wait("bad-2", "wait", { timestamp: "2000" })]);
  assert(invalidTimes[0].kind === "tool" && invalidTimes[0].event.waitBatch?.elapsedMs === undefined, "invalid timestamps omit elapsed time");
  const negativeTimes = reduceToolEvents([wait("late", "wait", { timestamp: "3000" }), wait("early", "wait", { timestamp: "1000" })]);
  assert(negativeTimes[0].kind === "tool" && negativeTimes[0].event.waitBatch?.elapsedMs === 0, "negative elapsed time clamps to zero");
  const sessions = reduceToolEvents([wait("session-a", "wait", { sourceSessionId: "a" }), wait("session-b", "wait", { sourceSessionId: "b" })]);
  assert(sessions.length === 2, "waits from different sessions never merge");
  const unknownSessions = reduceToolEvents([
    wait("unknown-a", "wait", { sourceSessionId: undefined }),
    wait("unknown-b", "wait", { sourceSessionId: undefined }),
  ]);
  assert(unknownSessions.length === 2, "waits without session identity never merge");
  const knownReduced = reduceToolEvents([wait("duplicate", "wait", { sourceSessionId: "same" }), wait("duplicate-next", "wait", { sourceSessionId: "same" })]);
  const recomputed = coalesceWaitEvents(knownReduced);
  assert(recomputed.length === 1 && recomputed[0].kind === "tool" && recomputed[0].event.waitBatch?.members.length === 2, "recoalescing settled output does not duplicate members");
}

console.log("ok - wait batch predicate");
