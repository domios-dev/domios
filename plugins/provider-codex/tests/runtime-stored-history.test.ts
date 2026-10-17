import assert from "node:assert/strict";
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import type { ChatMsg, JsonValue, RuntimeContext } from "@codeterm/plugin-sdk";
import { runtimeStoredHistory, runtimeStoredRecords, storedHistoryAvailable } from "../src/runtimeStoredHistory";
import { loadRecordPlugin, plain, recordInput } from "../../provider-shared/tests/recordHookFixture";
import { runtimePlan, runtimeResponse } from "../src/runtime";
import { collectHistory } from "./runtimeHistoryHost";
import { object } from "../src/runtimeProtocol";
import { createTranscriptState, reduceTranscript, transcriptRows } from "../../../packages/chat-core/src/transcript";
import type { NormalizedChatMessage } from "../../../packages/chat-core/src/types";

const fixture = JSON.parse(fs.readFileSync(new URL("./fixtures/recorded-runtime-history.json", import.meta.url), "utf8")) as {
  sessionId:string; records:JsonValue[]; runtimeTurns:JsonValue[]; expectedIds:string[];
};
const base = JSON.parse(fs.readFileSync(new URL("./fixtures/runtime/tool-call.json", import.meta.url), "utf8"));
const ctx: RuntimeContext = {...base.context, threadId:fixture.sessionId};
const operation = {kind:"readHistory" as const, threadId:fixture.sessionId};
const stored = runtimeStoredHistory(fixture.sessionId, fixture.records.map(record => JSON.stringify(record)).join("\n"));
const reopenedRecords = JSON.parse(fs.readFileSync(new URL("./fixtures/reopened-assistant-history.json", import.meta.url), "utf8")) as JsonValue[];
const reopenedText = reopenedRecords.map(record => JSON.stringify(record)).join("\n");
const reopenedInput = recordInput(reopenedText);
const bundle = loadRecordPlugin(fileURLToPath(new URL("..", import.meta.url)));
const reference = JSON.parse(fs.readFileSync(new URL("./fixtures/reference-runtime-history.json", import.meta.url), "utf8")) as {
  sessionId:string; records:JsonValue[];
};
const referenceText = reference.records.map(record => JSON.stringify(record)).join("\n");
assert.equal(storedHistoryAvailable(fixture.sessionId, JSON.stringify(fixture.records[0])), true);
for (const head of [referenceText, null, "{", JSON.stringify({type:"session_meta",payload:{id:"foreign-session"}})]) {
  assert.equal(storedHistoryAvailable(reference.sessionId, head), false);
}
const referenceInput = recordInput(referenceText);
for (const rows of [
  runtimeStoredHistory(reference.sessionId, referenceText),
  runtimeStoredRecords(reference.sessionId, referenceInput.records),
  bundle.runtimeStoredHistory!(reference.sessionId, referenceText),
  bundle.runtimeStoredRecords!(reference.sessionId, referenceInput.records, referenceInput.meta),
]) {
  assert.deepEqual(plain(rows.map(row => row.id)), ["child-item"]);
}
const opened: string[] = [];
const referenceBundle = loadRecordPlugin(fileURLToPath(new URL("..", import.meta.url)), {
  fileExists: () => true,
  readDir: () => [{name:"state_5.sqlite",path:"/tmp/.codex/state_5.sqlite",isFile:true,isDir:false,size:0,modifiedMs:null,createdMs:null}],
  sqliteQuery: <T>(_db:string, _sql:string, params?:unknown[]) => {
    assert.deepEqual(plain(params), [reference.sessionId]);
    return [{rollout_path:"/child.jsonl"}] as T[];
  },
  readFileHead: (path, limit) => {
    opened.push(path);
    assert.ok(limit <= 262144);
    return referenceText;
  },
});
assert.equal(referenceBundle.runtimeStoredHistoryAvailable!("/work/project", reference.sessionId), false);
assert.deepEqual(opened, ["/child.jsonl"]);
for (const rows of [
  runtimeStoredHistory("fixture-identity-1", reopenedText),
  runtimeStoredRecords("fixture-identity-1", reopenedInput.records),
  bundle.runtimeStoredHistory!("fixture-identity-1", reopenedText),
  bundle.runtimeStoredRecords!("fixture-identity-1", reopenedInput.records, reopenedInput.meta),
]) {
  assert.deepEqual(plain(rows.map(row => [row.id, row.type, row.content])), [
    ["fixture-identity-3", "assistant", "ACK-CODEX"],
    ["fixture-identity-6", "assistant", "CACK-3"],
    ["fixture-identity-9", "assistant", "ACK-FAST-MODEL"],
  ]);
  assert.deepEqual(plain(rows.map(row => row.toolFlowId)), ["fixture-identity-2", "fixture-identity-5", "fixture-identity-8"]);
}
const projected = runtimeStoredHistory(fixture.sessionId, JSON.stringify({
  type:"event_msg", payload:{type:"item_completed",thread_id:fixture.sessionId,turn_id:"large-turn",
    item:{id:"large-item",type:"AgentMessage",status:"completed",historyTruncated:true}},
}));
assert.equal(projected.length, 1);
assert.equal(projected[0].id, "large-item");
assert.equal(projected[0].type, "system");
assert.ok(projected[0].content.length > 0);
const projectedSnapshot = reduceTranscript(createTranscriptState(), {
  kind:"snapshot", authority:{sourceSessionId:fixture.sessionId,sessionBindingNonce:"own-launch"},
  range:{start:0,end:20 * 1024 * 1024,unit:"bytes"}, rows:projected as NormalizedChatMessage[],
});
assert.deepEqual(transcriptRows(projectedSnapshot.state).map(row => row.id), ["large-item"]);
const plan = runtimePlan(ctx,operation);
let step = runtimeResponse(ctx,operation,plan.state,{
  kind:"success",value:{data:fixture.runtimeTurns.map(turn => {
    const {items:_items,...metadata} = object(turn);
    return metadata;
  }),nextCursor:null} as JsonValue,
});
const chunks = [];
let calls = 0;
while (step.kind === "continue") {
  assert.ok(++calls <= fixture.runtimeTurns.length + 2);
  if (step.history) chunks.push(step.history);
  if (step.plan.action.kind !== "request") throw new Error("Expected bounded history request");
  const call = step.plan.action.call;
  const turn = fixture.runtimeTurns.map(object).find(turn => turn.id === object(call.params).turnId);
  step = runtimeResponse(ctx,operation,step.plan.state,{kind:"success",
    value:call.method === "thread/goal/get" ? {goal:null} : {data:turn!.items,nextCursor:null} as JsonValue});
}
assert.equal(step.kind,"complete");
if (step.kind !== "complete") throw new Error("History did not complete");
const live = collectHistory([...chunks,step.result]).items;
const positioned = (rows: ChatMsg[]) => rows.map((row, seq) => ({...row, seq}));
assert.deepEqual(stored.map(row => row.id), fixture.expectedIds);
assert.deepEqual(stored, positioned(live));

const repeated = [...fixture.records, fixture.records[4]];
assert.deepEqual(runtimeStoredHistory(fixture.sessionId, repeated.map(record => JSON.stringify(record)).join("\n")), stored);
assert.throws(() => runtimeStoredHistory("different-session", JSON.stringify(fixture.records[0])));
const sameText = structuredClone(fixture.records);
const userItems = sameText.map(record => object(object(record).payload).item).map(object).filter(item => item.type === "UserMessage");
userItems[1].content = userItems[0].content;
assert.deepEqual(runtimeStoredHistory(fixture.sessionId, sameText.map(record => JSON.stringify(record)).join("\n")).filter(row => row.type === "user").map(row => row.id), ["user-one", "user-two"]);
const authority = {sourceSessionId:fixture.sessionId, sessionBindingNonce:"own-launch"};
const snapshot = reduceTranscript(createTranscriptState(), {
  kind:"snapshot", authority, range:{start:0,end:4096,unit:"bytes"}, rows:stored as NormalizedChatMessage[],
});
const continued = reduceTranscript(snapshot.state, {
  kind:"delta", authority, range:null,
  rows:live.map(row => row.id === "msg_two" ? {...row,content:"continued answer"} : row) as NormalizedChatMessage[],
});
assert.deepEqual(transcriptRows(continued.state).map(row => row.id), fixture.expectedIds);
assert.equal(transcriptRows(continued.state).find(row => row.id === "msg_two")?.content, "continued answer");
console.log("PASS stored history: native/runtime projection parity, repeat-id dedupe, exact session, distinct equal-content items, live continuation");
