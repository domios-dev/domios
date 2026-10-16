import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { loadRecordPlugin, plain, recordInput } from "../../provider-shared/tests/recordHookFixture";
import type { JsonValue, SessionRecordsMeta } from "@codeterm/plugin-sdk";

const root = path.resolve(import.meta.dirname,"..");
const text = loadRecordPlugin(root), records = loadRecordPlugin(root);
const fixtures = fs.readdirSync(path.join(root,"tests/fixtures"),{recursive:true}).map(String).filter(file=>file.endsWith(".jsonl"));
for (const fixture of fixtures) {
  const input = fs.readFileSync(path.join(root,"tests/fixtures",fixture),"utf8");
  const parsed = recordInput(input,0,fixture);
  const old = text.parseSessionDelta!(input,{session_key:fixture,from_offset:0}) as {messages:unknown[]};
  const next = records.parseSessionRecords!(parsed.records,parsed.meta) as {messages:unknown[];new_offset:number};
  assert.deepEqual(plain(next.messages),plain(old.messages),fixture);
  assert.equal(next.new_offset,parsed.meta.newOffset);
  assert.deepEqual(plain(records.parseSessionMessageRecords!(parsed.records,parsed.meta)),plain(text.parseSessionMessages!(input)),fixture);
}
const runtime = JSON.parse(fs.readFileSync(path.join(root,"tests/fixtures/recorded-runtime-history.json"),"utf8")) as {sessionId:string;records:JsonValue[]};
const input = runtime.records.map(record=>JSON.stringify(record)).join("\n");
const parsed = recordInput(input);
assert.deepEqual(plain(records.runtimeStoredRecords!(runtime.sessionId,parsed.records,parsed.meta)),
  plain(text.runtimeStoredHistory!(runtime.sessionId,input)));
const large = {type:"event_msg",payload:{type:"item_completed",thread_id:"own",turn_id:"turn",
  item:{id:"large",type:"AgentMessage",status:"completed",text:"x".repeat(445_000)}}};
const largeText = JSON.stringify(large);
const largeInput = recordInput(largeText);
assert.deepEqual(plain(records.runtimeStoredRecords!("own",largeInput.records,largeInput.meta)),plain(text.runtimeStoredHistory!("own",largeText)));
const marker = {type:"event_msg",payload:{type:"item_completed",thread_id:"own",turn_id:"turn",
  item:{id:"large",type:"AgentMessage",status:"completed"}}};
const meta: SessionRecordsMeta = {...largeInput.meta,counts:{total:1,matched:1,invalid:0,oversized:1}};
assert.deepEqual(plain(records.runtimeStoredRecords!("own",[{offset:0,row:0,value:marker,oversized:true,bytes:largeText.length}],meta)),
  plain(text.runtimeStoredHistory!("own",JSON.stringify({...marker,payload:{...marker.payload,item:{...marker.payload.item,historyTruncated:true}}}))));
assert.throws(()=>records.runtimeStoredRecords!("foreign",parsed.records,parsed.meta));
console.log(`PASS records siblings: ${fixtures.length*2} transcript projections, captured runtime, 445 KB record, bounded marker, foreign session`);
