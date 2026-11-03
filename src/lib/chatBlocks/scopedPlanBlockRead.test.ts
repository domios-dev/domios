import assert from "node:assert/strict";
import test from "node:test";
import type { CommittedPlanChange } from "../../components/Canvas/planProjection";
import { createScopedPlanBlockRead } from "./scopedPlanBlockRead";

function deferred<T>(){let resolve!:(value:T)=>void;const promise=new Promise<T>((done)=>{resolve=done;});return {promise,resolve};}
async function flush(){for(let index=0;index<10;index++)await Promise.resolve();}
const change=(revision:string):CommittedPlanChange=>({plan_id:"chosen",revision,previous_revision:String(Number(revision)-1),task_ids:["task"],reset:false,deleted:false});

test("visible block reads wait for subscriptions and reject older committed responses",async()=>{
  const ready=deferred<void>();
  const slow=deferred<{revision:string;name:string}>();
  let changed!:(value?:CommittedPlanChange)=>void;
  let calls=0;
  const source=createScopedPlanBlockRead({
    epoch:()=>1,revision:(value:{revision:string;name:string})=>value.revision,
    watch:(callback)=>{changed=callback;return {ready:ready.promise,stop(){}};},
    read:async()=>{calls++;return calls===1 ? {revision:"1",name:"Initial"} : calls===2 ? slow.promise : {revision:"3",name:"Current"};},
  });
  source.read("chosen");assert.equal(calls,0);
  const release=source.retain("chosen");source.refresh("chosen");await flush();assert.equal(calls,0);
  ready.resolve();await flush();assert.equal(source.read("chosen").data?.name,"Initial");
  changed(change("2"));await flush();assert.equal(source.read("chosen").data?.name,"Initial");
  changed(change("3"));await flush();assert.equal(source.read("chosen").data?.name,"Current");
  slow.resolve({revision:"2",name:"Obsolete"});await flush();assert.equal(source.read("chosen").data?.name,"Current");
  changed(change("2"));await flush();assert.equal(calls,3);
  release();
});

test("hidden block releases cancel pending acquisition and cannot acquire data on later events",async()=>{
  let changed!:(value?:CommittedPlanChange)=>void;
  let signal!:AbortSignal;let calls=0;let stopped=0;
  const pending=deferred<{revision:string}>();
  const source=createScopedPlanBlockRead({epoch:()=>1,read:async(_key:string,current:AbortSignal)=>{signal=current;calls++;return pending.promise;},watch:(callback)=>{changed=callback;return {ready:Promise.resolve(),stop(){stopped++;}};}});
  const release=source.retain("chosen");source.refresh("chosen");await flush();assert.equal(calls,1);
  release();assert.equal(signal.aborted,true);assert.equal(stopped,1);
  changed(change("2"));pending.resolve({revision:"1"});await flush();assert.equal(calls,1);assert.equal(source.read("chosen").data,null);
});

test("authority denial clears the visible block and reconnect recovers only its retained key",async()=>{
  let changed!:(value?:CommittedPlanChange)=>void;let calls=0;
  const source=createScopedPlanBlockRead({epoch:()=>1,
    read:async()=>{calls++;if(calls===2)throw Object.assign(new Error("Denied"),{status:403});return {revision:String(calls)};},
    watch:(callback)=>{changed=callback;return {ready:Promise.resolve(),stop(){}};},
  });
  const release=source.retain("chosen");source.refresh("chosen");await flush();assert.ok(source.read("chosen").data);
  changed(change("2"));await flush();assert.equal(source.read("chosen").data,null);assert.equal(source.read("chosen").error,"Denied");
  changed();await flush();assert.equal(source.read("chosen").data?.revision,"3");assert.equal(calls,3);release();
});

test("a peer epoch masks old data immediately and recovers the same visible scope",async()=>{
  let epoch=1;let calls=0;
  const source=createScopedPlanBlockRead({epoch:()=>epoch,read:async()=>({revision:"1",peer:++calls}),watch:()=>({ready:Promise.resolve(),stop(){}})});
  const release=source.retain("chosen");source.refresh("chosen");await flush();assert.equal(source.read("chosen").data?.peer,1);
  epoch=2;assert.equal(source.read("chosen").data,null);await flush();assert.equal(source.read("chosen").data?.peer,2);release();
});
