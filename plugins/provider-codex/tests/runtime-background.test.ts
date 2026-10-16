import assert from "node:assert/strict";
import {test} from "node:test";
import fs from "node:fs";
import vm from "node:vm";
import type {AgentRuntime,Operation,RpcReply,RuntimeContext,RuntimeAction,JsonValue} from "@codeterm/plugin-sdk";
import {runtimePlan,runtimeResponse} from "../src/runtime";

const fixture = JSON.parse(fs.readFileSync(new URL("./fixtures/background-terminals.json",import.meta.url),"utf8")) as {
  cases:{context:RuntimeContext;operation:Extract<Operation,{kind:"backgroundProcesses"}>;action:RuntimeAction;reply:RpcReply}[];
};
const sandbox = vm.createContext({module:{exports:{}},exports:{},host:new Proxy({}, {get:()=>{throw Error("Runtime accessed host");}})});
sandbox.exports=sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js",import.meta.url),"utf8"),sandbox);
const bundle = sandbox.module.exports.default as AgentRuntime;
const plain = <T>(value:T):T => JSON.parse(JSON.stringify(value));

const controlled = JSON.parse(fs.readFileSync(new URL("./fixtures/background-terminals-controlled.json",import.meta.url),"utf8")) as typeof fixture;
const captures = [
  ["listener",fixture,{processId:"23832",itemId:"exec-e32cc25f-84cd-4aa5-b3b3-608ede538960",command:"node -e \"require('net').createServer().listen(0,'127.0.0.1')\"",cwd:"/work/capture",osPid:null}],
  ["controlled",controlled,{processId:"4271",itemId:"exec-98f58094-9da7-4c57-9143-2cfc561c7323",command:"node -e \"const s=require('net').connect(54289,'127.0.0.1');s.on('close',()=>process.exit(0));s.on('error',()=>process.exit(1))\"",cwd:"/work/capture",osPid:null}],
] as const;
for (const [capture,fixture,expectedProcess] of captures) {
for (const [name,hooks] of [["source",{runtimePlan,runtimeResponse}],["bundle",bundle]] as const) {
  test(`${capture}/${name}: recorded list/terminate/clean shapes retain exact authority`,()=>{
    const results=fixture.cases.map(entry=>{
      const plan=hooks.runtimePlan(entry.context,entry.operation);
      assert.equal(plan.action.kind,"request");assert.equal(entry.action.kind,"request");
      if (plan.action.kind !== "request" || entry.action.kind !== "request") throw Error("Missing request");
      const {target,...wire}=plain(plan.action.call);
      assert.deepEqual(wire,entry.action.call);
      assert.deepEqual(target,{threadId:entry.operation.threadId});
      const step=plain(hooks.runtimeResponse(entry.context,entry.operation,plan.state,entry.reply));
      assert.equal(step.kind,"complete");
      if(step.kind !== "complete")throw Error("Missing completion");
      return step.result;
    });
    assert.deepEqual(results[0],{kind:"backgroundProcesses",threadId:fixture.cases[0].operation.threadId,processes:[expectedProcess],nextCursor:null});
    assert.deepEqual(results[1],{kind:"backgroundProcessTermination",threadId:fixture.cases[1].operation.threadId,processId:expectedProcess.processId,acknowledged:true});
    assert.deepEqual(results[2],{kind:"backgroundProcesses",threadId:fixture.cases[2].operation.threadId,processes:[],nextCursor:null});
    assert.deepEqual(results[3],{kind:"done"});
    assert.deepEqual(results[4],{kind:"backgroundProcesses",threadId:fixture.cases[4].operation.threadId,processes:[],nextCursor:null});
  });

  test(`${capture}/${name}: pagination is explicit and malformed or foreign data fails closed`,()=>{
    const {context,operation}=fixture.cases[0];
    const respond=(value:unknown)=>hooks.runtimeResponse(context,operation,null,{kind:"success",value:plain(value) as JsonValue});
    const page=plain(respond({data:[],nextCursor:"next"}));
    assert.equal(page.kind,"complete");
    if(page.kind === "complete")assert.deepEqual(page.result,{kind:"backgroundProcesses",threadId:operation.threadId,processes:[],nextCursor:"next"});
    for(const value of [{},{data:[]},{data:[],nextCursor:""},{data:[{}],nextCursor:null},
      {data:Array(257).fill({}),nextCursor:null}])assert.throws(()=>respond(value));
    assert.throws(()=>hooks.runtimePlan(context,{...operation,threadId:"foreign"}));
    const descendant={...context,threadParents:[{threadId:"child",parentThreadId:operation.threadId}]};
    assert.equal(hooks.runtimePlan(descendant,{...operation,threadId:"child"}).action.kind,"request");
    assert.throws(()=>hooks.runtimeResponse(context,{...operation,action:{kind:"list",cursor:"next"}},null,{kind:"success",value:{data:[],nextCursor:"next"}}));
    const result=hooks.runtimeResponse(context,operation,null,{kind:"error",error:{code:-32600,message:"thread not found: foreign"}});
    assert.equal(result.kind,"refused");
    assert.equal(hooks.runtimeResponse(context,operation,null,{kind:"error",error:{code:-32603,message:`thread not found: ${operation.threadId}`}}).kind,"refused");
  });
}

}
