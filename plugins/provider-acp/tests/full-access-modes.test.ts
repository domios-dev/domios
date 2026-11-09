import assert from "node:assert/strict";
import fs from "node:fs";
import type { JsonValue, Operation, RuntimeContext } from "@codeterm/plugin-sdk";
import { runtimePlan, runtimeResponse } from "../src/runtime";

interface Recording {
  package:string;
  currentModeId:string;
  availableModes:string[];
  launchProfiles?:string[];
  modeConfigOption?:string;
}
const read = <T>(path:string):T => JSON.parse(fs.readFileSync(new URL(path,import.meta.url),"utf8")) as T;
const recordings = read<Record<string,Recording>>("./fixtures/advertised-session-modes.json");
const runtime = read<{capabilities:{agentRuntime:{
  registryDefaults:Record<string,{fullAccessMode?:string}>;
  launchProfiles:Record<string,{fullAccessMode?:string}>;
}}}>("../plugin.json").capabilities.agentRuntime;

const declarations = [
  ...Object.entries(runtime.registryDefaults).map(([id,entry]) => ({id,mode:entry.fullAccessMode,recording:recordings[id]})),
  ...Object.entries(runtime.launchProfiles).map(([id,entry]) => ({
    id,mode:entry.fullAccessMode,recording:Object.values(recordings).find(recording=>recording.launchProfiles?.includes(id)),
  })),
].filter((declaration):declaration is {id:string;mode:string;recording:Recording|undefined} => declaration.mode !== undefined);
assert.ok(declarations.length >= 4,"every declared full-access mode is checked");

const base = {
  epoch:1,cwd:"/work/project",hostVersion:"1.19.1",bindingNonce:"nonce-fixture",
  settings:{},claimedSends:[],activeTurns:[],threadParents:[],
} as unknown as RuntimeContext;

for (const {id,mode,recording} of declarations) {
  assert.ok(recording,`${id} declares a full-access mode without a recorded handshake`);
  assert.ok(recording.availableModes.includes(mode),`${id} declares ${mode}; ${recording.package} advertises ${recording.availableModes.join(", ")}`);
  const context = {...base,tabId:`recorded-${id}`,providerSettings:{fullAccessMode:mode}} as RuntimeContext;
  const initialize:Operation = {kind:"initialize"};
  runtimeResponse(context,initialize,runtimePlan(context,initialize).state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
  const open:Operation = {kind:"openThread",instructions:"",settings:{}};
  const modes:JsonValue = {currentModeId:recording.currentModeId,availableModes:recording.availableModes.map(mode=>({id:mode,name:mode}))};
  const option = (currentValue:string):JsonValue => ({id:recording.modeConfigOption ?? "",name:"Mode",category:"mode",type:"select",currentValue,options:recording.availableModes.map(value=>({value,name:value}))});
  const step = runtimeResponse(context,open,runtimePlan(context,open).state,{kind:"success",value:{sessionId:`${id}-session`,modes,...(recording.modeConfigOption ? {configOptions:[option(recording.currentModeId)]} : {})}});
  assert.ok(step.kind === "continue" && step.plan.action.kind === "request",`${id} must request its declared mode`);
  if (step.kind !== "continue" || step.plan.action.kind !== "request") throw new Error("unreachable");
  if (!recording.modeConfigOption) {
    assert.equal(step.plan.action.call.method,"session/set_mode");
    assert.deepEqual(step.plan.action.call.params,{sessionId:`${id}-session`,modeId:mode});
    continue;
  }
  assert.equal(step.plan.action.call.method,"session/set_config_option");
  assert.deepEqual(step.plan.action.call.params,{sessionId:`${id}-session`,configId:recording.modeConfigOption,value:mode});
  const loaded = runtimeResponse(context,open,step.plan.state,{kind:"success",value:{configOptions:[option(mode)]}});
  assert.ok(loaded.kind === "complete" && loaded.result.kind === "threadLoaded",`${id} loads once its option moved`);
  if (loaded.kind !== "complete" || loaded.result.kind !== "threadLoaded") throw new Error("unreachable");
  // The reply carries only the options, so this is the report the host settles on one mode.
  assert.equal(loaded.result.settings.mode,recording.currentModeId,id);
  assert.equal(loaded.result.settings.configOptions?.find(entry=>entry.category === "mode")?.currentValue,mode,id);
}
console.log("✓ ACP declared full-access modes are the ones each agent advertises in its recorded handshake, and the plugin enters them");
