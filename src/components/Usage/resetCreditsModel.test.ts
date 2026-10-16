import assert from "node:assert/strict";
import {ResetCreditsModel, resetCreditsState} from "./resetCreditsModel";

async function main() {
  assert.deepEqual(resetCreditsState(undefined), {visible:false,disabled:true});
  const empty = {availableCount:0,nextExpiry:null,credits:null};
  const available = {...empty,availableCount:2};
  assert.deepEqual(resetCreditsState(empty), {visible:true,disabled:true});
  const keys:string[] = [];
  let uuidCalls = 0;
  let fail = true;
  const model = new ResetCreditsModel(() => `key-${++uuidCalls}`, async key => {
    keys.push(key);
    if (fail) {fail=false; throw new Error("uncertain response");}
    return {outcome:"reset"};
  });
  await model.confirm(); await model.retry();
  model.request(empty); await model.confirm();
  assert.deepEqual(keys, []);
  model.request(available); model.cancel(); await model.confirm();
  assert.equal(uuidCalls,0);
  model.request(available);
  assert.equal(uuidCalls,0);
  await model.confirm();
  assert.equal(model.key,"key-1");
  await model.retry();
  assert.deepEqual(keys,["key-1","key-1"]);
  assert.equal(model.outcome,"reset");
  model.request(available); await model.confirm();
  assert.deepEqual(keys,["key-1","key-1","key-2"]);
  let finish!:(value:{outcome:"noCredit"})=>void;
  const held = new ResetCreditsModel(() => "one", () => new Promise(resolve => {finish=resolve;}));
  held.request(available);
  const pending = held.confirm();
  await held.confirm(); await held.retry();
  assert.equal(held.busy,true);
  finish({outcome:"noCredit"}); await pending;
  assert.equal(held.outcome,"noCredit");
  console.log("PASS reset model: visibility, zero count, cancellation, confirmation, retry UUID and duplicate exclusion");
}
void main();
