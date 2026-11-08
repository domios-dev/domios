import assert from "node:assert/strict";
import type {JsonValue,Operation,RuntimeContext,RuntimeStep} from "@codeterm/plugin-sdk";
import {runtimePlan,runtimeResponse} from "../src/runtime";

const methods=[{id:"opaque",name:"Agent sign-in"}];
const initialized={protocolVersion:1,agentCapabilities:{sessionCapabilities:{resume:{}}},authMethods:methods};
const context=(tabId:string):RuntimeContext=>({tabId,epoch:1,bindingNonce:"nonce",cwd:"/fixture",hostVersion:"1.19.1",settings:{},providerSettings:{},claimedSends:[],activeTurns:[],threadParents:[]});
function reply(ctx:RuntimeContext,op:Operation,value:JsonValue) {
  return runtimeResponse(ctx,op,runtimePlan(ctx,op).state,{kind:"success",value});
}
function states(step:RuntimeStep):string[] {
  return step.kind === "complete" ? (step.events ?? []).flatMap(event=>event.kind === "authentication" ? [event.authentication.state] : []) : [];
}
const init:Operation={kind:"initialize"};
const open:Operation={kind:"openThread",instructions:"",settings:{}};
const auth:Operation={kind:"control",action:"auth",value:null};
const ctx=context("successful-session");
assert.deepEqual(states(reply(ctx,init,initialized)),["unknown"]);
const loaded=reply(ctx,open,{sessionId:"own"});
assert.deepEqual(states(loaded),["authenticated"]);
if (loaded.kind !== "complete") throw new Error("Expected successful session");
const event=loaded.events?.find(event=>event.kind === "authentication");
assert.deepEqual(event && event.kind === "authentication" ? event.authentication.methods : [],[{id:"opaque",name:"Agent sign-in",available:true}]);
assert.deepEqual(states(reply(ctx,auth,initialized)),["authenticated"]);

const resumed=context("successful-resume");
reply(resumed,init,initialized);
assert.deepEqual(states(reply({...resumed,threadId:"restored"},{kind:"resumeThread",threadId:"restored",settings:{},instructions:""},{sessionId:"restored"})),["authenticated"]);

const bound={...ctx,threadId:"own"};
runtimeResponse(bound,auth,{method:"initialize"},{kind:"error",error:{code:-32000,message:"Authentication required"}});
assert.deepEqual(states(reply(bound,auth,initialized)),["needs_auth"]);
const reattach:Operation={kind:"resumeThread",threadId:"own",settings:{},instructions:""};
const local=runtimePlan(bound,reattach);
assert.equal(local.action.kind,"local");
assert.deepEqual(states(runtimeResponse(bound,reattach,local.state,{kind:"success",value:null})),[]);
assert.deepEqual(states(reply(bound,auth,initialized)),["needs_auth"]);
const prompt:Operation={kind:"submitTurn",threadId:"own",sendId:"opaque-send",input:{text:"opaque task",attachments:[]},settings:{}};
const prompting=runtimePlan(bound,prompt);
assert.throws(()=>runtimeResponse({...bound,threadId:"foreign"},{...prompt,threadId:"foreign"},prompting.state,{kind:"success",value:{stopReason:"end_turn"}}));
assert.deepEqual(states(reply(bound,auth,initialized)),["needs_auth"]);
assert.deepEqual(states(runtimeResponse(bound,prompt,prompting.state,{kind:"success",value:{stopReason:"end_turn"}})),["authenticated"]);
assert.deepEqual(states(reply(bound,auth,initialized)),["authenticated"]);

const failed=context("failed-session");
reply(failed,init,initialized);
const opening=runtimePlan(failed,open);
const refusal=runtimeResponse(failed,open,opening.state,{kind:"error",error:{code:-32000,message:"Authentication required"}});
assert.equal(refusal.kind,"refused");
assert.deepEqual(states(reply(failed,auth,initialized)),["needs_auth"]);
const nextEpoch={...ctx,epoch:2};
assert.deepEqual(states(reply(nextEpoch,init,initialized)),["unknown"]);
const anonymous=context("no-declaration");
reply(anonymous,init,{...initialized,authMethods:[]});
assert.deepEqual(states(reply(anonymous,open,{sessionId:"own"})),[]);
console.log("✓ ACP successful new/resume/own turn authenticates; failures, foreign sessions and fresh epochs do not");
