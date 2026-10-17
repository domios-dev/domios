import assert from "node:assert/strict";
import { codexEffectiveSettings, codexServiceTier } from "../src/runtimeSettings";
import { modelServiceTiers, tierAvailable, serviceTierSelection } from "../src/runtimeServiceTiers";

for (const id of [null,"priority","ultrafast"]) {
  assert.deepEqual(codexServiceTier({serviceTier:{id}}),{serviceTier:id});
  assert.deepEqual(codexEffectiveSettings({}, {thread:{serviceTier:id}}).serviceTier,{id});
  assert.equal(codexEffectiveSettings({serviceTier:{id}},{}).serviceTier,undefined);
}
assert.deepEqual(codexServiceTier({}),{});
assert.equal(serviceTierSelection({id:false}),false);
assert.equal(serviceTierSelection(null),false);
const catalogue = [{model:"model",serviceTiers:[{id:"priority",name:"Fast",description:"fast"},{id:"ultrafast",name:"Ultrafast",description:"ultra"}]}];
const allowed = modelServiceTiers(catalogue,{requirements:null});
assert.equal(tierAvailable({model:"model",serviceTierAvailability:allowed},{id:"ultrafast"}),true);
for (const requirements of [undefined,{requirements:{featureRequirements:{ultrafast_mode:false}}},{requirements:{featureRequirements:{fast_mode:false}}}]) {
  assert.equal(tierAvailable({model:"model",serviceTierAvailability:modelServiceTiers(catalogue,requirements)},{id:"ultrafast"}),false);
}
assert.equal(tierAvailable({model:"model",serviceTierAvailability:modelServiceTiers([{model:"model",serviceTiers:[]}],{})},{id:"ultrafast"}),false);
assert.equal(tierAvailable({},{id:"ultrafast"}),false);
assert.equal(tierAvailable({},{id:null}),true);
assert.equal(tierAvailable({model:"model",serviceTierAvailability:modelServiceTiers(catalogue,{supportsIndependentSpeedModes:true,requirements:{featureRequirements:{fast_mode:false}}})},{id:"ultrafast"}),true);
console.log("runtime tier mapping, confirmation and availability passed");
