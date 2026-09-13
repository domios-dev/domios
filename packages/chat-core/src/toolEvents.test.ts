import { materializeToolEvent, reduceToolEvents } from "./toolEvents";
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
    call("send", "send-call", { toolName: "exec_command", toolSummary: 'codeterm send --tab tab "hello"' }),
  ]);
  assert(items.length === 1, "internal orchestration calls do not enter the chat timeline");
  assert(items[0].kind === "tool" && items[0].event.summary === 'codeterm send --tab tab "hello"', "real codeterm send remains visible");
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
  const session = { sourceSessionId: "session" };
  const described = (id: string, flowId: string | undefined, description: string | undefined) => call(id, id, {
    toolKind: "command", toolSummary: `cmd ${id}`, toolDetail: `cmd ${id}`, toolFlowId: flowId, toolDescription: description, ...session,
  });
  const linked = reduceToolEvents([described("a", "msg-1", "First"), described("b", "msg-1", "Second"), result("ra", "a", "out a")]);
  assert(linked.length === 1 && linked[0].kind === "tool" && linked[0].event.stream?.members.length === 2, "described calls sharing a proven flow form one stream");
  assert(linked[0].kind === "tool" && linked[0].event.summary === "Second", "stream title rolls to the latest description");
  assert(linked[0].kind === "tool" && linked[0].event.stream?.members[0].result === "out a", "members keep their exact output");
  const unlinked = reduceToolEvents([described("c", undefined, "Only"), described("d", "msg-2", "Alone")]);
  assert(unlinked.length === 2 && unlinked.every((item) => item.kind === "tool" && !item.event.stream), "unlinked or singleton described calls stay rows");
  const partial = reduceToolEvents([described("e", "msg-3", "Described"), described("f", "msg-3", undefined)]);
  assert(partial.length === 2, "a flow with an unlabeled member stays individual rows");

  const boundary = (content: "start" | "end", flowId: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage => ({
    id: `flow-${content}-${flowId}`, type: "tool_flow", content, toolFlowId: flowId, ...session, ...patch,
  });
  const titled = (id: string, title: string, patch: Partial<NormalizedChatMessage> = {}) => call(id, id, { toolTitle: title, toolError: false, ...session, ...patch });
  const turns = reduceToolEvents([
    boundary("start", "turn-a"),
    titled("t1", "npm test"),
    { id: "between", type: "assistant", content: "between", ...session },
    titled("t2", "Wait for command output"),
    titled("t3", "wait_agent", { toolName: "wait_agent" }),
    boundary("end", "turn-a"),
    boundary("start", "turn-b"),
    titled("t4", "git status"),
    titled("t5", "Read a.ts"),
  ]);
  assert(turns.length === 3, "each turn yields one stream while text keeps its order");
  assert(turns[0].kind === "tool" && turns[0].event.stream?.members.map((member) => member.id).join() === "t1,t2,t3", "wait follow-ups are ordinary actions of their turn");
  assert(turns[0].kind === "tool" && turns[0].event.stream?.active === false, "a closed settled turn is inactive");
  assert(turns[1].kind === "message", "assistant text is not absorbed into a stream");
  assert(turns[2].kind === "tool" && turns[2].event.stream?.members.map((member) => member.id).join() === "t4,t5", "a later turn never merges with an earlier one");
  assert(turns[2].kind === "tool" && turns[2].event.stream?.active === true, "an open turn keeps its stream active");
  const foreign = reduceToolEvents([boundary("start", "turn-x"), titled("s1", "a", { sourceSessionId: "other" }), titled("s2", "b", { sourceSessionId: "other" })]);
  assert(foreign.length === 2, "a boundary never assigns calls from another session");
  assert(reduceToolEvents([call("wait-agent", "wait-agent", { toolName: "wait_agent" })]).length === 1, "wait_agent is an ordinary visible action");
}

console.log("ok - tool activity streams");
