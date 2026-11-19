import assert from "node:assert/strict";
import fs from "node:fs";
import type {FileDiff} from "@codeterm/chat-core";
import {partToChatMsg, type OpencodePart} from "../src/helpers";
import {toolId, v2Content, v2History} from "../src/runtimeV2Projection";

interface Fixture {
  source: string;
  parts: Array<Record<string, unknown>>;
}

const fixture=JSON.parse(fs.readFileSync(new URL("./fixtures/runtime-v2-tool-cards.json",import.meta.url),"utf8")) as Fixture;
const assertRow=(part:Record<string,unknown>, ordinal=0)=>{
  const row=v2Content("tool-card-message",part,ordinal,ordinal+1,1795000000000);
  assert.ok(row,"tool fixture should project");
  return row;
};
const firstDiff=(row:{fileDiffs?:unknown}):FileDiff|undefined=>Array.isArray(row.fileDiffs) ? row.fileDiffs[0] as FileDiff : undefined;
const byId=(id:string)=>{
  const part=fixture.parts.find((entry)=>entry.id === id);
  assert.ok(part,`missing fixture row ${id}`);
  return part;
};

assert.match(fixture.source,/OpenCode CLI 2\.3\.0.*session_message/);

const expected=[
  ["call-shell","command","Get-ChildItem src"],
  ["call-read","read","src/example.ts"],
  ["call-edit","edit","src/example.ts"],
  ["call-edit-single-diff","edit","src/other.ts"],
  ["call-write","edit","src/new.ts"],
  ["call-grep","search","needle"],
  ["call-glob","search","src/**/*.ts"],
  ["call-webfetch","fetch","https://example.invalid"],
  ["call-todowrite","task","Update tasks"],
  ["call-skill","read","example-skill"],
  ["call-question","task","Ask question"],
] as const;

for(const [id,kind,summary] of expected) {
  const row=assertRow(byId(id));
  assert.equal(row.toolKind,kind,id);
  assert.equal(row.toolSummary,summary,id);
  assert.equal(row.toolError,false,id);
}

for(const id of ["call-edit","call-edit-single-diff"]) {
  const diff=firstDiff(assertRow(byId(id)));
  assert.ok(diff,`${id} should expose a parsed file diff`);
  assert.equal(diff.path,id === "call-edit" ? "src/example.ts" : "src/other.ts");
  assert.equal(diff.kind,"modify");
  assert.equal(diff.additions,1);
  assert.equal(diff.deletions,1);
  assert.deepEqual(diff.hunks[0]?.lines,id === "call-edit" ? ["-old value","+new value"] : ["-before","+after"]);
}
const created=firstDiff(assertRow(byId("call-write")));
assert.equal(created?.path,"src/new.ts");
assert.equal(created?.kind,"add");

const open131Write=assertRow(byId("call-open131-write"));
const open131CreateDiff=firstDiff(open131Write);
assert.equal(open131Write.toolKind,"edit");
assert.equal(open131CreateDiff?.path,"C:/Users/User/AppData/Local/Temp/domios-open131-1d8cadc0/cards-created.txt");
assert.equal(open131CreateDiff?.kind,"add");
assert.equal(open131CreateDiff?.additions,1);
assert.equal(open131CreateDiff?.deletions,0);
assert.equal(open131Write.toolDurationMs,10);

const open131Edit=assertRow(byId("call-open131-edit"));
const open131EditDiff=firstDiff(open131Edit);
assert.equal(open131EditDiff?.path,open131CreateDiff?.path);
assert.equal(open131EditDiff?.kind,"modify");
assert.equal(open131EditDiff?.additions,1);
assert.equal(open131EditDiff?.deletions,1);
assert.deepEqual(open131EditDiff?.hunks[0]?.lines,["-PUBLIC_OPEN131_CREATED","+PUBLIC_OPEN131_EDITED"]);
assert.equal(open131Edit.toolDurationMs,10);
assert.notEqual(open131Write.id,open131Edit.id,"distinct native calls retain distinct row identities");

const applyPatch=v2Content("patch-message",{
  id:"call-apply-patch",
  type:"tool",
  name:"apply_patch",
  state:{status:"completed",input:{input:"--- a/src/patched.ts\n+++ b/src/patched.ts\n@@ -1 +1 @@\n-old\n+new"},content:[{type:"text",text:"patch applied"}],metadata:{files:[]}},
},0,1);
assert.equal(applyPatch?.toolKind,"edit");
assert.equal(applyPatch?.toolSummary,"src/patched.ts");
assert.equal(applyPatch && firstDiff(applyPatch)?.path,"src/patched.ts");

for(const [id,status,error] of [
  ["call-aborted-search","interrupted",undefined],
  ["call-failed-shell","failed",true],
] as const) {
  const row=assertRow(byId(id));
  assert.equal(row.toolStatus,status,id);
  assert.equal(row.toolError,error,id);
}
assert.equal(assertRow(byId("call-aborted-search")).toolInterrupted,true);

const timedOut={...byId("call-shell"),state:{...byId("call-shell").state as object,metadata:{timeout:true}}};
const timeoutRow=assertRow(timedOut);
assert.equal(timeoutRow.toolStatus,"failed");
assert.equal(timeoutRow.toolError,true);
assert.equal(timeoutRow.toolResult,"example output");

for(const id of ["call-shell","call-edit"]) {
  const part=byId(id), state=part.state as Record<string,unknown>;
  const v1=partToChatMsg({type:"tool",tool:part.name,callID:part.id,state} as OpencodePart,"assistant",String(part.id),1795000000000) as Record<string,unknown>;
  const v2=assertRow(part);
  const fields=v2 as unknown as Record<string,unknown>;
  for(const field of ["toolKind","toolSummary","toolDetail","toolEdits","fileDiffs","toolError"]) assert.deepEqual(fields[field],v1[field],`${id} ${field} v1/v2 parity`);
}

const streaming={...byId("call-shell"),state:{status:"streaming",input:'{"command":"pnpm check"}'}};
const streamingRow=assertRow(streaming);
assert.equal(streamingRow.toolStatus,"running");
assert.equal((streamingRow.toolInput as Record<string,unknown> | undefined)?.command,"pnpm check");

const createdAt=1795000000000;
const history=v2History([{id:"history-message",type:"assistant",time:{created:createdAt},content:fixture.parts}],"asc");
assert.equal(history.length,fixture.parts.length);
history.forEach((row,index)=>{
  assert.equal(row.id,toolId("history-message",String(fixture.parts[index].id)));
  assert.equal(row.seq,index+1);
  assert.equal(row.timestamp,new Date(createdAt).toISOString());
});

console.log("OpenCode v2 tool card projection cases passed");
