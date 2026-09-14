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
  const titledOnly = reduceToolEvents([
    call("g", "g", { toolFlowId: "msg-4", toolTitle: "Read a.ts", ...session }),
    call("h", "h", { toolFlowId: "msg-4", toolTitle: "Grep TODO", ...session }),
  ]);
  assert(titledOnly.length === 2 && titledOnly.every((item) => item.kind === "tool" && !item.event.stream), "call-stamped flows never stream on titles alone");

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
  assert(turns.length === 4, "text between calls splits the turn while keeping source order");
  assert(turns[0].kind === "tool" && !turns[0].event.stream && turns[0].event.id === "t1", "a call before text stays its own row");
  assert(turns[1].kind === "message", "assistant text is not absorbed into a stream");
  assert(turns[2].kind === "tool" && turns[2].event.stream?.members.map((member) => member.id).join() === "t2,t3", "wait follow-ups are ordinary actions of their run");
  assert(turns[2].kind === "tool" && turns[2].event.stream?.active === false, "a closed settled turn is inactive");
  assert(turns[3].kind === "tool" && turns[3].event.stream?.members.map((member) => member.id).join() === "t4,t5", "a later turn never merges with an earlier one");
  assert(turns[3].kind === "tool" && turns[3].event.stream?.active === true, "an open turn keeps its stream active");
  assert(turns[3].kind === "tool" && turns[3].event.status === "pending", "an open turn heads as running even when its recorded calls settled");
  assert(turns[2].kind === "tool" && turns[2].event.status === "success", "a closed settled turn heads as done");

  const split = reduceToolEvents([
    boundary("start", "turn-s"),
    titled("s1", "npm test"),
    titled("s2", "git status"),
    { id: "inbound", type: "user", content: "STATUS: in_review", inboundKind: "report", fromLabel: "Hopper", ...session },
    titled("s3", "npm run lint"),
    titled("s4", "git diff"),
  ]);
  assert(split.length === 3 && split[1].kind === "message", "an inbound row closes the stream above it");
  assert(split[0].kind === "tool" && split[0].event.stream?.members.map((member) => member.id).join() === "s1,s2", "the earlier run keeps its own calls");
  assert(split[0].kind === "tool" && split[0].event.status === "success" && split[0].event.stream?.active === false, "a closed run of settled calls never inherits its open turn's spinner");
  assert(split[2].kind === "tool" && split[2].event.stream?.members.map((member) => member.id).join() === "s3,s4", "later calls of the same turn start a new stream below");
  assert(split[2].kind === "tool" && split[2].event.status === "pending", "the latest run of an open turn stays running");

  const running = reduceToolEvents([
    boundary("start", "turn-r"),
    call("r1", "r1", { toolTitle: "npm test", ...session }),
    call("r2", "r2", { toolTitle: "git status", ...session }),
    { id: "text-r", type: "assistant", content: "waiting", ...session },
    result("r1-out", "r1", "12 passed", session),
  ]);
  assert(running[0].kind === "tool" && running[0].event.status === "pending" && running[0].event.stream?.members[1].status === "pending", "a closed run heads as running only while its own calls run");
  assert(running[0].kind === "tool" && running[0].event.stream?.members[0].status === "success", "a late result settles its call in place");

  const duplicated = reduceToolEvents([
    boundary("start", "turn-d"),
    call("d1", "dup", { toolTitle: "npm test", ...session }),
    call("d2", "other", { toolTitle: "git status", ...session }),
    call("d1-snapshot", "dup", { toolTitle: "npm test", ...session }),
    result("dup-out", "dup", "12 passed", session),
  ]);
  assert(duplicated.length === 1 && duplicated[0].kind === "tool" && duplicated[0].event.stream?.members.map((member) => member.toolId).join() === "dup,other", "a stream shows one member per call id");
  assert(duplicated[0].kind === "tool" && duplicated[0].event.stream?.members[0].result === "12 passed", "the kept member carries the call's latest state");
  const foreign = reduceToolEvents([boundary("start", "turn-x"), titled("s1", "a", { sourceSessionId: "other" }), titled("s2", "b", { sourceSessionId: "other" })]);
  assert(foreign.length === 2, "a boundary never assigns calls from another session");
  assert(reduceToolEvents([call("wait-agent", "wait-agent", { toolName: "wait_agent" })]).length === 1, "wait_agent is an ordinary visible action");
}

{
  const stale = { sourceSessionId: "stale" };
  const start = (flowId: string): NormalizedChatMessage => ({ id: `start-${flowId}`, type: "tool_flow", content: "start", toolFlowId: flowId, ...stale });
  const titled = (id: string, patch: Partial<NormalizedChatMessage> = {}) => call(id, id, { toolTitle: `title ${id}`, ...stale, ...patch });
  const [abandoned, open] = reduceToolEvents([
    start("turn-a"), titled("a1", { toolError: false }), titled("a2"),
    start("turn-b"), titled("b1"), titled("b2"),
  ]);
  assert(abandoned.kind === "tool" && abandoned.event.stream?.members[1].status === "unresolved", "a call without output is unresolved once a later turn starts");
  assert(abandoned.kind === "tool" && abandoned.event.status === "unresolved" && abandoned.event.stream?.active === false, "the superseded stream header is static");
  assert(open.kind === "tool" && open.event.status === "pending" && open.event.stream?.active === true, "the latest open turn keeps its running state");

  const claude = { sourceSessionId: "claude" };
  const superseded = reduceToolEvents([call("c1", "c1", claude), { id: "u1", type: "user", content: "next", seq: 1, ...claude }]);
  assert(superseded[0].kind === "tool" && superseded[0].event.status === "unresolved", "a pending call superseded by a later user turn is unresolved");
  const live = reduceToolEvents([call("c5", "c5", claude), { id: "u5", type: "user", content: "live", seq: 5, clientOrder: 9, ...claude }]);
  assert(live[0].kind === "tool" && live[0].event.status === "unresolved", "a live source user row supersedes even though arrival stamps clientOrder");
  const optimistic = reduceToolEvents([call("c2", "c2", claude), { id: "u2", type: "user", content: "typed", clientOrder: 1, correlationId: "u2", ...claude }]);
  assert(optimistic[0].kind === "tool" && optimistic[0].event.status === "pending", "an optimistic user row is not source evidence");
  const late = reduceToolEvents([call("c3", "c3", claude), { id: "u3", type: "user", content: "next", seq: 3, ...claude }, result("r3", "c3", "done", claude)]);
  assert(late[0].kind === "tool" && late[0].event.status === "success", "a result that does arrive still resolves the call");
  const foreign = reduceToolEvents([call("c4", "c4", claude), { id: "u4", type: "user", content: "elsewhere", seq: 4, sourceSessionId: "other" }]);
  assert(foreign[0].kind === "tool" && foreign[0].event.status === "pending", "another session's user turn never supersedes a call");
}

{
  const turn = { sourceSessionId: "echo" };
  const start: NormalizedChatMessage = { id: "echo-start", type: "tool_flow", content: "start", toolFlowId: "turn-e", ...turn };
  const titledCall = (id: string, patch: Partial<NormalizedChatMessage> = {}) => call(id, id, { toolTitle: `title ${id}`, toolError: false, ...turn, ...patch });
  const echoed = reduceToolEvents([start, titledCall("e1"), titledCall("echo-1", { toolEcho: true, toolFlowId: "turn-e" }), titledCall("e2")]);
  assert(echoed.length === 1 && echoed[0].kind === "tool" && echoed[0].event.stream?.members.map((member) => member.id).join() === "e1,e2", "a stream drops activity echoes of its own commands");
  const loose = reduceToolEvents([
    titledCall("e3", { sourceSessionId: "loose", toolFlowId: "turn-x" }),
    titledCall("echo-2", { sourceSessionId: "loose", toolFlowId: "turn-x", toolEcho: true }),
  ]);
  assert(loose.length === 2, "without a formed stream an echo stays a visible record");
}

{
  const session = { sourceSessionId: "codex-compound" };
  const pipeline = "codeterm mem secret get --name admin-key | python -c \"import sys\nprint(len(sys.stdin.read()))\"";
  const launcherArgv = ["C:\\Program Files\\PowerShell\\7\\pwsh.exe", "-Command", pipeline];
  const exec = call("exec", "exec-call", { toolKind: "command", toolSummary: pipeline, toolDetail: pipeline, toolInput: { raw: "tools.exec_command(...)" }, ...session });
  const echo = (id: string, patch: Partial<NormalizedChatMessage> = {}) => call(id, id, {
    toolName: "command_execution",
    toolKind: "command",
    toolSummary: launcherArgv.join(" "),
    toolDetail: launcherArgv.join(" "),
    toolInput: { command: launcherArgv.join(" "), argv: launcherArgv },
    toolEcho: true,
    toolError: false,
    ...session,
    ...patch,
  });
  const folded = reduceToolEvents([exec, result("exec-out", "exec-call", "Script completed", session), echo("echo-1", { toolExitCode: 3, toolDurationMs: 900 })]);
  assert(folded.length === 1, "an echo re-reporting its call's launcher-wrapped command renders once");
  assert(folded[0].kind === "tool" && folded[0].event.id === "exec", "the call keeps its own identity");
  assert(folded[0].kind === "tool" && folded[0].event.exitCode === 3 && folded[0].event.durationMs === 900, "the echo's outcome belongs to its call");
  assert(folded[0].kind === "tool" && folded[0].event.status === "error", "a nonzero echoed exit marks the call failed");

  const lateResult = reduceToolEvents([exec, echo("echo-2"), result("echo-2-out", "echo-2", "42", { ...session, toolExitCode: 0 })]);
  assert(lateResult.length === 1, "an echo's separate result row does not resurrect the echo");
  assert(lateResult[0].kind === "tool" && lateResult[0].event.result === "42" && lateResult[0].event.exitCode === 0, "an echo's later result lands on its call");

  const unrelated = reduceToolEvents([exec, echo("echo-3", { toolSummary: "git status", toolDetail: "git status", toolInput: { argv: ["git", "status"] } })]);
  assert(unrelated.length === 2, "an echo of a different command stays a visible record");

  const nextTurn = reduceToolEvents([exec, { id: "u-next", type: "user", content: "next", seq: 9, ...session }, echo("echo-4")]);
  assert(nextTurn.length === 3, "an echo never folds into a call from an earlier turn");
}

console.log("ok - tool activity streams");
