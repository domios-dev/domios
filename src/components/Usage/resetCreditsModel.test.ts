import assert from "node:assert/strict";
import {ResetCreditsModel, resetCreditsState, resetCreditsForTab, clearResetCreditsForTab, resetBalanceLabel, availableResetCredits, resetCreditLabel} from "./resetCreditsModel";

async function main() {
  assert.deepEqual(resetCreditsState(undefined), {visible:false,disabled:true});
  const empty = {availableCount:0,nextExpiry:null,credits:null};
  const available = {...empty,availableCount:2};
  assert.equal(resetBalanceLabel(2,1),"Reset credits: 2 before · 1 after");
  assert.equal(resetBalanceLabel(2,null),"Reset credits: 2 before · unconfirmed after");
  const observed = new ResetCreditsModel(() => "observed", async () => ({outcome:"reset",remainingCount:1}));
  observed.request(available);
  await observed.confirm();
  assert.equal(observed.beforeCount,2);
  assert.equal(observed.afterCount,1);
  const unconfirmed = new ResetCreditsModel(() => "unconfirmed", async () => ({outcome:"reset"}));
  unconfirmed.request(available);
  await unconfirmed.confirm();
  assert.equal(unconfirmed.afterCount,null);
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
  const duplicate = held.retry();
  assert.equal(held.busy,true);
  finish({outcome:"noCredit"}); await Promise.all([pending, duplicate]);
  assert.equal(held.outcome,"noCredit");
  const sent: string[] = [];
  let shouldFail = true;
  const consume = async (key: string) => {sent.push(key); if (shouldFail) {shouldFail=false;throw new Error("uncertain");} return {outcome:"alreadyRedeemed" as const};};
  const original = resetCreditsForTab("remounted-tab", () => "retained-key", consume);
  original.request(available); await original.confirm();
  const remounted = resetCreditsForTab("remounted-tab", () => "wrong-new-key", consume);
  assert.equal(remounted.key,"retained-key");
  assert.equal(resetCreditsForTab("other-tab", () => "other", consume).key,null);
  await remounted.retry();
  assert.deepEqual(sent,["retained-key","retained-key"]);
  assert.equal(remounted.key,null);
  assert.equal(remounted.inFlight,null);
  assert.equal(remounted.busy,false);
  let settle!:(value:{outcome:"reset"})=>void;
  const outstanding = resetCreditsForTab("in-flight-tab", () => "one-key", () => new Promise(resolve=>{settle=resolve;}));
  outstanding.request(available); const outstandingRequest=outstanding.confirm();
  const again=resetCreditsForTab("in-flight-tab", () => "wrong", consume);
  assert.equal(again.inFlight,outstanding.inFlight);
  assert.equal(again.busy,true);
  let notifications=0; const unsubscribe=again.subscribe(()=>notifications++);
  settle({outcome:"reset"}); await outstandingRequest;
  assert.equal(again.key,null); assert.ok(notifications>0); unsubscribe();
  clearResetCreditsForTab("remounted-tab");
  assert.notEqual(resetCreditsForTab("remounted-tab", () => "after-sign-out", consume),original);
  let settleSignedOut!:(value:{outcome:"reset"})=>void;
  const signedOut = resetCreditsForTab("sign-out-pending", () => "old-account-key", () => new Promise(resolve=>{settleSignedOut=resolve;}));
  signedOut.request(available); const oldRequest=signedOut.confirm();
  clearResetCreditsForTab("sign-out-pending");
  const signedIn = resetCreditsForTab("sign-out-pending", () => "new-account-key", consume);
  assert.notEqual(signedIn,signedOut); assert.equal(signedIn.key,null);
  settleSignedOut({outcome:"reset"}); await oldRequest;
  assert.equal(signedOut.outcome,null); assert.equal(signedIn.key,null);
  const syncFailure = new ResetCreditsModel(() => "sync-key", () => {throw new Error("synchronous transport failure");});
  syncFailure.request(available); await syncFailure.confirm();
  assert.equal(syncFailure.key,"sync-key"); assert.equal(syncFailure.inFlight,null); assert.equal(syncFailure.busy,false);
  const credits = {...available,credits:[
    {id:"first",title:"Weekly grant",status:"available" as const,expiresAt:2000000000000},
    {id:"second",status:"available" as const,expiresAt:null},
    {id:"used",status:"redeemed" as const,expiresAt:null},
  ]};
  assert.deepEqual(availableResetCredits(credits).map(credit => credit.id),["first","second"]);
  assert.equal(availableResetCredits({...credits,credits:[...credits.credits,credits.credits[0]]}).length,2);
  assert.equal(resetCreditLabel(credits.credits[0]),"Weekly grant");
  assert.equal(resetCreditLabel(credits.credits[1]),"second");
  const selectedCalls: (string|undefined)[][] = [];
  let failSelection = true;
  const selection = new ResetCreditsModel(() => "selected-key",async (key,id) => {
    selectedCalls.push([key,id]);
    if (failSelection) {failSelection=false;throw new Error("uncertain");}
    return {outcome:"reset"};
  });
  selection.select(credits,"missing"); assert.equal(selection.selectedCreditId,null);
  selection.select(credits,"second"); assert.equal(selection.selectedCreditId,"second");
  assert.equal(resetCreditsState(available,"second").disabled,true);
  selection.request(available); assert.equal(selection.confirming,false);
  selection.request(credits); selection.select(credits,"first");
  assert.equal(selection.pendingCredit?.id,"second");
  await selection.confirm(); selection.select(credits,"first"); await selection.retry();
  assert.deepEqual(selectedCalls,[["selected-key","second"],["selected-key","second"]]);
  selection.select(credits,null); selection.request(credits); await selection.confirm();
  assert.equal(selectedCalls[2][1],undefined);
  console.log("PASS reset model: visibility, zero count, cancellation, confirmation, retry UUID and duplicate exclusion");
}
void main();
