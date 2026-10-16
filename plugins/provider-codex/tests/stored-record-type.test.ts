import assert from "node:assert/strict";
import { runtimeStoredHistory, storedRecordType } from "../src/runtimeStoredHistory";

for (const line of [
  '{"timestamp":"2026-10-16","type":"event_msg","payload":{}}',
  '{"type":"event_msg","timestamp":"2026-10-16","payload":{}}',
  '{"type":"event_msg","payload":{}}',
  JSON.stringify({timestamp:'escaped "type": {', type:"event_msg", payload:{}}),
  ' { "timestamp" : "now", "type" : "event_msg", "payload": {} }',
]) assert.equal(storedRecordType(line), "event_msg", line);
assert.equal(storedRecordType('{"timestamp":"now","payload":{"type":"event_msg"}}'), undefined);
assert.equal(storedRecordType('{"payload":{"type":"event_msg"},"type":"event_msg"}'), undefined);
assert.equal(storedRecordType('{"type":"compacted","payload":{"type":"event_msg"}}'), "compacted");
assert.equal(storedRecordType('{"type":"session_meta","payload":{"id":"own"}}'), "session_meta");
assert.equal(storedRecordType('{"metadata":[{"type":"event_msg"}]}'), undefined);
assert.equal(storedRecordType(''), undefined);
const ignored = JSON.stringify({timestamp:"now",type:"compacted",payload:{type:"event_msg",text:"x".repeat(445_000)}});
const nestedOnly = '{"payload":{"type":"event_msg"}}';
const reordered = JSON.stringify({payload:{type:"item_completed",thread_id:"own",turn_id:"turn",
  item:{id:"late-type",type:"AgentMessage",historyTruncated:true}},type:"event_msg"});
assert.equal(storedRecordType(reordered), undefined);
const parse = JSON.parse;
const parsed: string[] = [];
try {
  JSON.parse = (value, reviver) => { parsed.push(value); return parse(value, reviver); };
  assert.deepEqual(runtimeStoredHistory("own", ignored), []);
  assert.deepEqual(parsed, []);
  assert.deepEqual(runtimeStoredHistory("own", nestedOnly), []);
  assert.deepEqual(parsed, [nestedOnly]);
  const rows = runtimeStoredHistory("own", `\n  \n${reordered}\n`);
  assert.deepEqual(rows.map(row => ({id:row.id,type:row.type,turn:row.toolFlowId})),
    [{id:"late-type",type:"system",turn:"turn"}]);
  assert.deepEqual(parsed, [nestedOnly,reordered]);
  assert.throws(() => runtimeStoredHistory("own", '{"type":"session_meta","payload":{"id":"foreign"}}'));
  assert.equal(parsed.length, 3);
  assert.throws(() => runtimeStoredHistory("own", '{"payload":{"id":"foreign"},"type":"session_meta"}'));
  assert.equal(parsed.length, 4);
} finally { JSON.parse = parse; }
console.log("PASS stored record prefix: writer key orders, escaped strings, nested-only type rejection");
