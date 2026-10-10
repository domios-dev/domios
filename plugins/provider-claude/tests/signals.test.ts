import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import {authStatusProbe, parseAuthStatus, parseSessionObservations} from "../src/signals";

let checks = 0;
function check(run: () => void) { run(); checks++; }
check(() => assert.deepEqual(authStatusProbe(), {args:["auth","status"], timeoutMs:3000}));
const status = {loggedIn:true,authMethod:"oauth",subscriptionType:"max",configDirectory:"fixture"};
check(() => assert.deepEqual(parseAuthStatus(status), status));
check(() => assert.equal(parseAuthStatus({loggedIn:"true"}), null));
check(() => assert.equal(parseAuthStatus({loggedIn:true,authMethod:8}), null));
check(() => assert.deepEqual(parseAuthStatus({loggedIn:false}), {loggedIn:false,authMethod:null,subscriptionType:null,configDirectory:null}));
check(() => assert.deepEqual(parseSessionObservations({isApiErrorMessage:true,error:"rate_limit"}), {api_error:"rate_limit",permission_mode:null,clears_api_error:false}));
check(() => assert.equal(parseSessionObservations({error:"authentication_error"}), null));
check(() => assert.equal(parseSessionObservations({isApiErrorMessage:true,error:{type:"authentication_error"}})?.api_error, "authentication"));
check(() => assert.equal(parseSessionObservations({permissionMode:"plan"})?.permission_mode, "plan"));
check(() => assert.equal(parseSessionObservations({permissionMode:"auto"})?.permission_mode, "auto"));
check(() => assert.equal(parseSessionObservations({permissionMode:"opaque"}), null));
check(() => assert.equal(parseSessionObservations({type:"assistant",message:{}})?.clears_api_error, true));
const sandbox = {module:{exports:{} as {default?: typeof import("../src/plugin").default}},exports:{},host:{}};
vm.runInNewContext(fs.readFileSync(new URL("../plugin.js",import.meta.url),"utf8"),sandbox);
const plugin = sandbox.module.exports.default!;
check(() => assert.deepEqual(JSON.parse(JSON.stringify(plugin.parseAuthStatus!(status))), status));
const row = {type:"assistant",uuid:"own",permissionMode:"plan",isApiErrorMessage:true,error:"rate_limit",message:{content:[{type:"text",text:"opaque"}]}};
const delta = plugin.parseSessionDelta!(JSON.stringify(row)) as {messages:Array<{provider_observations:unknown}>};
check(() => assert.deepEqual(JSON.parse(JSON.stringify(delta.messages[0].provider_observations)), parseSessionObservations(row)));
check(() => {
  const metadata = plugin.parseSessionDelta!(JSON.stringify({type:"metadata",sessionId:"own",permissionMode:"plan"})) as {messages:Array<{provider_observations:{permission_mode:string}}>};
  assert.equal(metadata.messages[0].provider_observations.permission_mode,"plan");
});
check(() => {
  const metadata = plugin.parseSessionDelta!(JSON.stringify({type:"assistant",sessionId:"own",isApiErrorMessage:true,error:"rate_limit",message:{content:[]}})) as {messages:Array<{provider_observations:{api_error:string}}>};
  assert.equal(metadata.messages[0].provider_observations.api_error,"rate_limit");
});
console.log(`${checks} passed, 0 failed`);
