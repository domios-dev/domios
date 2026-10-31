import assert from "node:assert/strict";
import fs from "node:fs";
import { codexEffectiveSettings, codexServiceTier, codexThreadConfig, codexNeedsWindowsSandboxSelection, codexWindowsSandboxOverride } from "../src/runtimeSettings";
import { modelServiceTiers, tierAvailable, serviceTierSelection } from "../src/runtimeServiceTiers";

const hostOwnedConfigFixture = JSON.parse(fs.readFileSync(new URL("../../../tests/fixtures/app-server-host-owned-config.json",import.meta.url),"utf8")) as Record<string,boolean>;

assert.equal(codexNeedsWindowsSandboxSelection({config:{}}),true);
for (const sandbox of ["mxc","elevated","unelevated"]) assert.equal(codexNeedsWindowsSandboxSelection({config:{windows:{sandbox}}}),false);
for (const prefer_mxc of [false,true]) assert.equal(codexNeedsWindowsSandboxSelection({config:{features:{prefer_mxc}}}),false);
assert.equal(codexNeedsWindowsSandboxSelection({config:{windows:{allow_mxc:false}}}),false);
for (const config of [undefined,null,[],{windows:{sandbox:"unknown"}},{windows:{allow_mxc:"unknown"}},{features:{prefer_mxc:"unknown"}}])
  assert.throws(()=>codexNeedsWindowsSandboxSelection({config}));
assert.equal(codexWindowsSandboxOverride({requirements:null}),"mxc");
assert.equal(codexWindowsSandboxOverride({requirements:{allowedWindowsSandboxImplementations:["elevated","mxc"],allowedSandboxModes:["workspace-write"]}}),"mxc");
assert.equal(codexWindowsSandboxOverride({requirements:{allowedWindowsSandboxImplementations:null,allowedSandboxModes:null}}),"mxc");
for (const requirements of [
  {allowedWindowsSandboxImplementations:["elevated"],allowedSandboxModes:["workspace-write"]},
  {allowedWindowsSandboxImplementations:[],allowedSandboxModes:["workspace-write"]},
  {allowedWindowsSandboxImplementations:["mxc"],allowedSandboxModes:["read-only"]},
  {allowedWindowsSandboxImplementations:["mxc"],allowedSandboxModes:["workspace-write"],defaultPermissions:"opaque-profile"},
  {allowedWindowsSandboxImplementations:["mxc"],allowedSandboxModes:["workspace-write"],featureRequirements:{prefer_mxc:false}},
]) assert.equal(codexWindowsSandboxOverride({requirements}),undefined);
for (const requirements of [undefined,[],{},
  {allowedWindowsSandboxImplementations:["unknown"],allowedSandboxModes:["workspace-write"]},
  {allowedWindowsSandboxImplementations:["mxc"],allowedSandboxModes:"unknown"},
  {allowedWindowsSandboxImplementations:["mxc"],allowedSandboxModes:["workspace-write"],featureRequirements:[]},
]) assert.throws(()=>codexWindowsSandboxOverride({requirements}));

for (const id of [null,"priority","ultrafast"]) {
  assert.deepEqual(codexServiceTier({serviceTier:{id}}),{serviceTier:id});
  assert.deepEqual(codexEffectiveSettings({}, {thread:{serviceTier:id}}).serviceTier,{id});
  assert.equal(codexEffectiveSettings({serviceTier:{id}},{}).serviceTier,undefined);
}
assert.deepEqual(codexServiceTier({}),{});
const hostOwnedConfig = codexThreadConfig({}, {questions:true,todos:false});
assert.deepEqual(Object.fromEntries(Object.entries(hostOwnedConfig).filter(([key])=>key.startsWith("features."))),hostOwnedConfigFixture);
assert.equal(codexThreadConfig({}, {questions:false,todos:false})["features.default_mode_request_user_input"],undefined);
for (const reply of [{serviceTier:"default"},{thread:{serviceTier:"default"}}]) {
  const confirmed = codexEffectiveSettings({model:"next-model",serviceTier:{id:"priority"}},reply);
  assert.deepEqual(confirmed.serviceTier,{id:null});
  assert.equal(tierAvailable(confirmed,confirmed.serviceTier!),true);
}
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
assert.equal(modelServiceTiers([{model:"model",serviceTiers:[]}],{requirements:{featureRequirements:{ultrafast_mode:false}}}).model.find(tier => tier.id === "ultrafast")?.unavailableReason,"Disabled by your administrator");
assert.equal(tierAvailable({model:"model",serviceTierAvailability:modelServiceTiers(catalogue,{supportsIndependentSpeedModes:true,requirements:{featureRequirements:{fast_mode:false}}})},{id:"ultrafast"}),true);
console.log("runtime tier mapping, confirmation and availability passed");
