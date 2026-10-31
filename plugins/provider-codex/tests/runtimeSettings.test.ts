import assert from "node:assert/strict";
import fs from "node:fs";
import { codexEffectiveSettings, codexServiceTier, codexThreadConfig, codexNeedsWindowsSandboxSelection, codexWindowsSandboxOverride, codexRestrictedCliConfig, codexValidateCliTrust, codexRestrictedCliInstructions } from "../src/runtimeSettings";
import type {RuntimeContext} from "@codeterm/plugin-sdk";
import {runtimePlan as sourcePlan, runtimeResponse as sourceResponse} from "../src/runtime";
import type {Operation, RuntimePlan, RpcReply} from "@codeterm/plugin-sdk";
import { modelServiceTiers, tierAvailable, serviceTierSelection } from "../src/runtimeServiceTiers";

const hostOwnedConfigFixture = JSON.parse(fs.readFileSync(new URL("../../../tests/fixtures/app-server-host-owned-config.json",import.meta.url),"utf8")) as Record<string,boolean>;

const cliContext: RuntimeContext = {
  tabId:"own-tab",bindingNonce:"own-binding",epoch:4,cwd:"C:/opaque/project",hostVersion:"1.18.0",
  settings:{mode:"workspace-write",networkAccess:false},providerSettings:{},activeTurns:[],threadParents:[],claimedSends:[],
  restrictedCli:{tabId:"own-tab",bindingNonce:"own-binding",epoch:4,cwd:"C:/opaque/project",metadataDirectory:".codex",executable:"C:/trusted/cli.exe",
    executableSha256:"a".repeat(64),configSha256:"b".repeat(64),ruleSha256:"c".repeat(64)},
};
assert.deepEqual(codexRestrictedCliConfig(cliContext),{projects:{"C:/opaque/project":{trust_level:"trusted"}}});
assert.deepEqual(codexRestrictedCliConfig({...cliContext,restrictedCli:undefined}),{});
assert.deepEqual(codexRestrictedCliConfig({...cliContext,restrictedCli:undefined,settings:{mode:"full-access"},providerSettings:{restrictedCli:cliContext.restrictedCli as never}}),{});
for (const changed of [
  {tabId:"foreign"},{bindingNonce:"foreign"},{epoch:3},{cwd:"C:/foreign"},{metadataDirectory:".other"},{metadataDirectory:"../escape"},{executable:"relative.exe"},
  {executableSha256:"unknown"},{ruleSha256:"unknown"},{configSha256:"unknown"},
  {executable:"C:/unsafe path/cli.exe"},
]) assert.throws(()=>codexRestrictedCliConfig({...cliContext,restrictedCli:{...cliContext.restrictedCli!,...changed}}));
assert.throws(()=>codexRestrictedCliConfig({...cliContext,settings:{mode:"full-access",networkAccess:false}}));
assert.throws(()=>codexRestrictedCliConfig({...cliContext,settings:{mode:"workspace-write",networkAccess:true}}));
for (const config of [{},{projects:{"C:/opaque/project":{trust_level:"trusted"}}},{projects:{"C:/other":{trust_level:"untrusted"}}}])
  assert.doesNotThrow(()=>codexValidateCliTrust(cliContext,{config}));
for (const config of [{projects:{"C:/opaque/project":{trust_level:"untrusted"}}},{projects:{"C:\\opaque":{trust_level:"untrusted"}}},{projects:[]}])
  assert.throws(()=>codexValidateCliTrust(cliContext,{config}));
assert.throws(()=>codexValidateCliTrust(cliContext,{}));
const cliReplay = JSON.parse(fs.readFileSync(new URL("./fixtures/runtime/restricted-cli.json",import.meta.url),"utf8")) as {
  context:RuntimeContext;operations:{operation:Operation;context?:RuntimeContext}[];
  rpc:({direction:"outbound";operation:number;action:RuntimePlan["action"]}|{direction:"inbound";operation:number;reply:RpcReply})[];
  expected:{results:{operation:number;step:unknown}[]};
};
for (const [index,entry] of cliReplay.operations.entries()) {
  const ctx = entry.context ?? cliReplay.context;
  let plan = sourcePlan(ctx,entry.operation);
  let completed:unknown;
  for (const row of cliReplay.rpc.filter(row=>row.operation === index)) {
    if (row.direction === "outbound") assert.deepEqual(JSON.parse(JSON.stringify(plan.action)),row.action);
    else {
      const step = sourceResponse(ctx,entry.operation,plan.state,row.reply);
      if (step.kind === "continue") plan=step.plan;
      else completed=step;
    }
  }
  assert.deepEqual(JSON.parse(JSON.stringify(completed)),cliReplay.expected.results.find(row=>row.operation === index)?.step);
}

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

assert.equal(codexRestrictedCliInstructions({...cliContext,restrictedCli:undefined},"opaque"),"opaque");
assert.ok(codexRestrictedCliInstructions(cliContext,"opaque")!.startsWith("opaque\n\n"));
assert.ok(codexRestrictedCliInstructions(cliContext,"opaque")!.includes(cliContext.restrictedCli!.executable));
assert.throws(()=>codexRestrictedCliInstructions({...cliContext,restrictedCli:{...cliContext.restrictedCli!,executable:"C:/unsafe path/cli.exe"}}));
