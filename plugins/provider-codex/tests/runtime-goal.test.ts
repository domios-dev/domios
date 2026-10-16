import assert from "node:assert/strict";
import fs from "node:fs";
import { goalPlan, goalResponse, goalIntent } from "../src/runtimeGoal";
const fixture = JSON.parse(fs.readFileSync(new URL("./fixtures/goal-protocol.json",import.meta.url),"utf8"));
const thread = fixture.threadId;
for (const [action,method,status] of [["read","get",undefined],["cancel","clear",undefined],["pause","set","paused"],["resume","set","active"],["complete","set","complete"]] as const) {
  const plan = goalPlan(thread,{action});
  assert.deepEqual(plan.action,{kind:"request",call:{method:`thread/goal/${method}`,params:{threadId:thread,...(action !== "read" ? {origin:"user"} : {}),...(status ? {status} : {})}}});
  if (status) {
    assert.equal(goalResponse(thread,{goalAction:action},{kind:"success",value:{goal:{...fixture.goal,status}}}).kind,"complete");
    if (status !== "active") assert.equal(goalResponse(thread,{goalAction:action},{kind:"success",value:{goal:fixture.goal}}).kind,"refused");
  }
}
assert.deepEqual(goalIntent(fixture.goal,thread),{goal:"Fixture objective",goalStatus:"active"});
assert.equal(goalIntent(fixture.goal,"foreign"),null);
assert.equal(goalResponse(thread,{goalAction:"read"},{kind:"success",value:{goal:{...fixture.goal,threadId:"foreign"}}}).kind,"refused");
assert.equal(goalResponse(thread,{goalAction:"cancel"},{kind:"success",value:{cleared:false}}).kind,"refused");
const cleared = goalResponse(thread,{goalAction:"cancel"},{kind:"success",value:{cleared:true}});
assert.equal(cleared.kind,"continue");
assert.equal(goalResponse(thread,{goalAction:"verifyCancel"},{kind:"success",value:{goal:fixture.goal}}).kind,"refused");
assert.deepEqual(goalResponse(thread,{goalAction:"verifyCancel"},{kind:"success",value:{goal:null}}),{kind:"complete",result:{kind:"goal",threadId:thread,intent:null}});
assert.throws(()=>goalPlan(thread,{action:"invalid"}));
console.log("goal lifecycle fixture checks passed");
