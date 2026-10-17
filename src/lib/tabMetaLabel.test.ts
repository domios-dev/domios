import assert from "node:assert/strict";
import { confirmedTabServiceTier, serviceTierLabel, tabServiceTierAvailability, tabMetaParts } from "./tabMetaLabel";
import type { ProviderDescriptor, Tab } from "./types";

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
console.log("tab meta labels: 8 assertions passed");
