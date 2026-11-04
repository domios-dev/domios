import { test } from "vitest";
import assert from "node:assert/strict";
import { confirmedTabServiceTier, serviceTierLabel, tabServiceTierAvailability, tabMetaParts } from "./tabMetaLabel";
import { runtimeStateFromSnapshot } from "./agentRuntimeState";
import type { ProviderDescriptor, Tab } from "./types";

test("tab meta identity and confirmed service tier", () => {
  assert.deepEqual(tabMetaParts("model", "fast", "worker", "idle"), ["model", "fast", "worker", "idle"]);
  for (const fast of [undefined]) assert.deepEqual(tabMetaParts("model", fast, "worker", "idle"), ["model", "worker", "idle"]);
  const provider = { agent_runtime: { serviceTiers: [{id:"priority",name:"Fast",description:""},{id:"ultrafast",name:"Ultrafast",description:"",unavailableReason:"Not available on your plan"}] } } as unknown as ProviderDescriptor;
  const tab = { agent_session_id: "own", session_binding_nonce: "nonce", agent_runtime_config: {
    serviceTier: {id:"priority"}, confirmedServiceTier: { selection:{id:"priority"}, sessionId: "own", bindingNonce: "nonce" },
  } } as unknown as Tab;
  assert.deepEqual(confirmedTabServiceTier(tab, provider), {id:"priority"});
  assert.equal(serviceTierLabel(provider,{id:"ultrafast"}),"ultrafast");
  assert.equal(tabServiceTierAvailability(tab),undefined);
  assert.equal(confirmedTabServiceTier({ ...tab, agent_session_id: "foreign" }, provider), undefined);
  assert.equal(confirmedTabServiceTier({ ...tab, session_binding_nonce: "new" }, provider), undefined);
  assert.equal(confirmedTabServiceTier({ ...tab, agent_runtime_config: { serviceTier: {id:"priority"} } }, provider), undefined);
  assert.equal(confirmedTabServiceTier(tab, {} as ProviderDescriptor), undefined);

  assert.deepEqual(tabMetaParts(undefined, undefined, undefined, undefined, {agentName:"Wire Agent",binaryVersion:"0.0.1"}), ["Wire Agent","0.0.1"]);
  assert.deepEqual(tabMetaParts("confirmed", undefined, undefined, undefined, {agentName:"Wire Agent",binaryVersion:"0.0.1"}), ["Wire Agent","0.0.1","confirmed"]);
});


test("an explicit notice clear replaces the previous notice in the same epoch", () => {
  const previous = {state:"loaded" as const,epoch:1,notice:"Requested model was not applied"};
  assert.equal(runtimeStateFromSnapshot(previous,{state:"loaded",epoch:1,notice:null})?.notice, null);
});
