import assert from "node:assert/strict";
import {goalActions,goalMutation,goalStatusLabel,goalVisible,type GoalSnapshot} from "./goalModel";
for (const capability of [undefined, {}, { goals: { actions: ["read"] } }]) {
  for (const intent of [undefined, null, {}, { title: "Title only" }, { goal: "" }, { goal: " \n " }]) {
    assert.equal(goalVisible(capability, intent), false);
  }
  assert.equal(goalVisible(capability, { title: "Title", goal: " Objective " }), capability?.goals !== undefined);
}
const snapshot:GoalSnapshot={supported:true,binding:{threadId:"fixture",bindingNonce:"nonce",epoch:3},intent:{goal:"opaque",goalStatus:"active"},actions:["read","pause","resume","complete","cancel"]};
assert.deepEqual(goalActions(snapshot),["pause","complete","cancel"]);
assert.deepEqual(goalActions({...snapshot,intent:{goal:"opaque",goalStatus:"paused"}}),["resume","complete","cancel"]);
assert.deepEqual(goalActions({...snapshot,actions:["read"]}),[]);
assert.deepEqual(goalActions({supported:false}),[]);
assert.deepEqual(goalActions({...snapshot,intent:null}),[]);
assert.deepEqual(goalMutation(snapshot,"cancel"),{action:"cancel",confirmed:true,binding:snapshot.binding,expectedIntent:snapshot.intent});
assert.throws(()=>goalMutation({supported:false},"cancel"));
assert.equal(goalStatusLabel("usageLimited"),"Usage limited");
assert.equal(goalStatusLabel("budgetLimited"),"Budget limited");
console.log("goal action model passed");
