import assert from "node:assert/strict";
import {test} from "node:test";
import {cachedSlashCommands,capabilityBadge,type InitializeCapabilities} from "./providerCapabilities";
import {filterSlashCommands} from "./slashCommands";
const data:InitializeCapabilities={commands:[{name:"fixture",description:"opaque",argument_hint:"<input>"}],models:null,agents:null,account:{subscription_type:"fixture-plan",api_provider:"fixture-provider"},current_permission_mode:"plan",fast_mode_state:"on",output_style:null};
test("cached initialize commands carry argument hints and filter locally",()=>{
  const commands=cachedSlashCommands({status:"ready",data});
  assert.deepEqual(filterSlashCommands("/fi",commands),[{name:"/fixture",description:"opaque",argumentHint:"<input>"}]);
  assert.deepEqual(filterSlashCommands("/other",commands),[]);
});
test("unavailable and missing command capabilities do not guess autocomplete",()=>{
  assert.deepEqual(cachedSlashCommands({status:"unavailable",reason:"deadline"}),[]);
  assert.deepEqual(cachedSlashCommands({status:"pending"}),[]);
  assert.deepEqual(cachedSlashCommands({status:"ready",data:{...data,commands:null}}),[]);
});
test("badge exposes cached subscription and modes and typed failure",()=>{
  const badge=capabilityBadge({status:"ready",data});
  for (const value of ["fixture-plan","fixture-provider","plan","on"]) assert.ok(badge?.includes(value));
  assert.ok(capabilityBadge({status:"unavailable",reason:"deadline"})?.includes("deadline"));
  assert.equal(capabilityBadge(null),null);
});
