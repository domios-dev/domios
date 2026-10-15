import assert from "node:assert/strict";
import fs from "node:fs";
import type { ChatMsg, JsonValue, RuntimeContext } from "@codeterm/plugin-sdk";
import { runtimeStoredHistory } from "../src/runtimeStoredHistory";
import { runtimeResponse } from "../src/runtime";
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
const first = runtimeResponse(ctx, operation, {stage:"snapshot", seq:0}, {
  kind:"success", value:{thread:{id:fixture.sessionId, source:"cli", turns:fixture.runtimeTurns}} as JsonValue,
});
assert.equal(first.kind, "continue");
if (first.kind !== "continue") throw new Error("History did not request goal state");
const final = runtimeResponse(ctx, operation, first.plan.state, {kind:"success", value:{goal:null}});
assert.equal(final.kind, "complete");
if (final.kind !== "complete") throw new Error("History did not complete");
const live = collectHistory([...(first.history ? [first.history] : []), final.result]).items;
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
