import assert from "node:assert/strict";
import type { JsonValue, Operation, RpcReply, RuntimeStep } from "@codeterm/plugin-sdk";
import { pagedHistoryPlan, pagedHistoryResponse } from "../src/runtimeHistoryPager";
import { HISTORY_CHUNK_BYTES } from "../src/runtimeHistoryBounds";

type Read = Extract<Operation,{kind:"readHistory"}>;
const initial:Read={kind:"readHistory",threadId:"own-thread",order:"desc"};
const success=(value:unknown):RpcReply=>({kind:"success",value:value as JsonValue});
function reply(op:Read,state:JsonValue,value:unknown) { return pagedHistoryResponse(op,state as Record<string,unknown>,success(value)); }
function plan(op:Read) {
  const next=pagedHistoryPlan(op);
  if(next.action.kind!=="request")throw new Error("Missing RPC request");
  return {...next,params:next.action.call.params as Record<string,unknown>,method:next.action.call.method};
}
function drain(count:number,content:string) {
  let op=initial, maxBytes=0, pageCount=0, failed=false;
  const retained:string[]=[];
  for(;;) {
    let next=plan(op), step:RuntimeStep;
    if(next.method==="thread/turns/list") {
      assert.equal(next.params.limit,1);
      step=reply(op,next.state,{data:[{id:"one-large-turn",status:"completed"}],nextCursor:null});
      assert.equal(step.kind,"continue");
      if(step.kind!=="continue")throw new Error("Missing item request");
      next={...step.plan,params:step.plan.action.kind==="request" ? step.plan.action.call.params as Record<string,unknown> : {},method:"thread/items/list"};
    }
    if(next.method==="thread/goal/get") {
      step=pagedHistoryResponse(op,next.state as Record<string,unknown>,{kind:"error",error:{code:-32601,message:"optional"}});
    } else {
      assert.equal(next.params.sortDirection,"desc");assert.equal(next.params.limit,16);
      const start=Number(next.params.cursor ?? 0), end=Math.min(count,start+16);
      if(pageCount===8 && !failed) {
        const checkpoint=op.cursor;
        assert.throws(()=>pagedHistoryResponse(op,next.state as Record<string,unknown>,{kind:"error",error:{code:504,message:"fixture deadline"}}),
          error=>String((error as {message:string}).message).includes("fixture deadline"));
        assert.equal(op.cursor,checkpoint);failed=true;
      }
      step=reply(op,next.state,{data:Array.from({length:end-start},(_,offset)=>({item:{id:String(count-1-start-offset),type:"agentMessage",text:content}})),
        nextCursor:end<count ? String(end) : null});
    }
    if(step.kind!=="complete" || step.result.kind!=="historyPage")throw new Error("Missing bounded page");
    const bytes=Buffer.byteLength(JSON.stringify(step));maxBytes=Math.max(maxBytes,bytes);
    assert.ok(bytes<HISTORY_CHUNK_BYTES+16*1024);
    assert.ok(Buffer.byteLength(step.result.nextCursor ?? "")<4096);
    retained.unshift(...step.result.items.map(row=>row.id));pageCount++;
    if(!step.result.nextCursor)break;
    op={...initial,cursor:step.result.nextCursor};
    assert.ok(pageCount<count+3);
  }
  assert.deepEqual(retained,Array.from({length:count},(_,index)=>String(index)));
  assert.equal(new Set(retained).size,count);
  return {maxBytes,pageCount,failed};
}
const large=drain(512,"[content truncated: 1048576 bytes total]");
assert.equal(large.failed,true);
assert.ok(large.maxBytes<16*1024);
const split=drain(160,"\u0001".repeat(10000));
assert.ok(split.pageCount>160/16);
const first=plan(initial);
const turns=reply(initial,first.state,{data:[{id:"turn",status:"completed"}],nextCursor:null});
if(turns.kind!=="continue")throw new Error("No items");
const itemPage=reply(initial,turns.plan.state,{data:[],nextCursor:"repeat"});
if(itemPage.kind!=="complete" || itemPage.result.kind!=="historyPage")throw new Error("No cursor");
const repeated=plan({...initial,cursor:itemPage.result.nextCursor});
assert.throws(()=>reply(initial,repeated.state,{data:[],nextCursor:"repeat"}));
assert.throws(()=>pagedHistoryPlan({...initial,threadId:"foreign",cursor:itemPage.result.nextCursor}));
console.log(`PASS bounded history pager: 512 MiB logical history, ${large.pageCount} pages, max=${large.maxBytes} bytes; byte splits, page-9 retry, cursor refusal, binding, order and dedupe`);

let adaptive = turns.plan;
for (const limit of [8,4,2,1]) {
  const step = pagedHistoryResponse(initial, adaptive.state as Record<string,unknown>, {
    kind:"error",error:{code:-32097,message:"opaque",data:{historyPageLimit:limit}}
  });
  assert.equal(step.kind,"continue");
  if (step.kind !== "continue" || step.plan.action.kind !== "request") throw new Error("No adaptive request");
  assert.equal((step.plan.action.call.params as Record<string,unknown>).limit,limit);
  assert.equal((step.plan.action.call.params as Record<string,unknown>).cursor,null);
  adaptive = step.plan;
}
const projected = reply(initial,adaptive.state,{data:[{item:{id:"huge",type:"agentMessage",historyTruncated:true}}],nextCursor:"older"});
assert.equal(projected.kind,"complete");
if (projected.kind !== "complete" || projected.result.kind !== "historyPage") throw new Error("No projected page");
assert.equal(projected.result.items[0].id,"huge");
assert.match(projected.result.items[0].content,/truncated/);
assert.ok(Buffer.byteLength(JSON.stringify(projected)) < HISTORY_CHUNK_BYTES);
console.log("PASS adaptive limits 16 -> 8 -> 4 -> 2 -> 1, cursor retained, truncated row delivered");
