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
const parse = JSON.parse;
const parsed: string[] = [];
try {
  JSON.parse = (value, reviver) => { parsed.push(value); return parse(value, reviver); };
  assert.deepEqual(runtimeStoredHistory("own", `${ignored}\n{"payload":{"type":"event_msg"}}`), []);
  assert.deepEqual(parsed, []);
  assert.throws(() => runtimeStoredHistory("own", '{"type":"session_meta","payload":{"id":"foreign"}}'));
  assert.equal(parsed.length, 1);
} finally { JSON.parse = parse; }
console.log("PASS stored record prefix: writer key orders, escaped strings, nested-only type rejection");
