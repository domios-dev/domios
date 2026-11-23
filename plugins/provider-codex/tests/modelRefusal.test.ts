import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import type { AgentRuntime, Operation, RuntimeContext, RuntimeFrame } from "@codeterm/plugin-sdk";
import { modelRefusal } from "../src/modelRefusal";

const source = readFileSync(new URL("../plugin.js", import.meta.url), "utf8");
const sandbox = vm.createContext({module:{exports:{}}, exports:{}, host:new Proxy({}, {get:()=>{throw new Error("Runtime hooks accessed host");}})});
sandbox.exports = sandbox.module.exports;
vm.runInContext(source, sandbox);
const hooks = sandbox.module.exports.default as AgentRuntime;
const plain = <T>(value:T): T => JSON.parse(JSON.stringify(value));
const fixtureRoot = new URL("./fixtures/runtime/", import.meta.url);
const {model, rejection} = JSON.parse(readFileSync(new URL("../../../tests/fixtures/runtime-model-unavailable.json", import.meta.url), "utf8"));
const fixture = JSON.parse(readFileSync(new URL("captured/approval.json", fixtureRoot), "utf8")) as {context: RuntimeContext};
const context = {...fixture.context, threadId:"own", activeTurnId:undefined, settings:{model}};
const refusal = modelRefusal(rejection, model);
assert.equal(refusal?.model, model);
assert.equal(modelRefusal({message:"Unknown model preference"}, model), undefined);

const operation: Operation = {kind:"submitTurn", threadId:"own", sendId:"send", input:{text:"opaque", attachments:[]}, settings:{model}};
const step = plain(hooks.runtimeResponse(context, operation, hooks.runtimePlan(context, operation).state, {kind:"error", error:rejection}));
assert.deepEqual(step, {kind:"refused", beforeAcceptance:true, reason:{code:"model_unavailable", message:refusal?.message}});

const frames: RuntimeFrame[] = [
  {kind:"notification", method:"turn/started", params:{threadId:"own", turn:{id:"turn"}}},
  {kind:"notification", method:"turn/completed", params:{threadId:"own", turn:{id:"turn", status:"failed", error:rejection}}},
];
const {events} = plain(hooks.runtimeReduce(context, null, frames));
const row = events.find(event => event.kind === "chatItem");
assert.ok(row?.kind === "chatItem");
assert.equal(row.item.systemSubtype, "model_unavailable");
assert.equal(row.item.content, refusal?.message);
assert.deepEqual(row.item.toolInput, {model});
console.log("✓ Codex model refusal in direct replies and turn notifications");
