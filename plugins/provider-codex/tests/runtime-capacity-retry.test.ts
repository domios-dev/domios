import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { AgentRuntime, RuntimeContext, JsonValue, RuntimeFrame, RuntimeErrorSignal } from "@codeterm/plugin-sdk";

const sandbox = vm.createContext({ module: { exports: {} }, exports: {}, host: {} });
sandbox.exports = sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), sandbox);
const runtime = sandbox.module.exports.default as AgentRuntime;
const context: RuntimeContext = { epoch: 1, tabId: "tab", bindingNonce: "nonce", threadId: "own", cwd: "/fixture", hostVersion: "1.15.2", claimedSends: [], activeTurns: [], threadParents: [], settings: {}, providerSettings: {} };

let checks = 0;
function signal(info: JsonValue, limits?: unknown, message = "opaque diagnostic"): RuntimeErrorSignal {
  const frames: RuntimeFrame[] = limits ? [{kind:"notification",method:"account/rateLimits/updated",params:{rateLimits:limits as JsonValue}}] : [];
  frames.push({kind:"notification",method:"turn/completed",params:{threadId:"own",turn:{id:"turn",status:"failed",error:{message,codexErrorInfo:info}}}});
  const result = runtime.runtimeReduce(context, null, frames);
  const event = result.events.find(event => event.kind === "chatItem");
  if (event?.kind !== "chatItem") throw new Error("Expected error signal");
  assert.equal(event.threadId, "own");
  assert.equal(event.item.toolFlowId, "turn");
  checks++;
  return event.item.toolInput as RuntimeErrorSignal;
}
const schema = JSON.parse(fs.readFileSync(new URL("./fixtures/runtime-error-schema-0.164.0.json", import.meta.url), "utf8"));
const variants = schema.definitions.CodexErrorInfo.anyOf as {enum?:string[]; required?:string[]}[];
const codes = variants.flatMap(variant => variant.enum ?? variant.required ?? []);
const classification: Record<string, "transient" | "rate" | "none"> = {
  contextWindowExceeded:"none", sessionBudgetExceeded:"none", usageLimitExceeded:"rate", rateLimitExceeded:"rate",
  flexUnavailable:"transient", serverOverloaded:"transient", cyberPolicy:"none", misalignmentPolicyViolation:"none",
  tooManyDenials:"none", internalServerError:"transient", unauthorized:"none", badRequest:"none", threadRollbackFailed:"none", sandboxError:"none", other:"none",
  httpConnectionFailed:"transient", responseStreamConnectionFailed:"transient", responseStreamDisconnected:"transient", responseTooManyFailedAttempts:"transient", activeTurnNotSteerable:"none",
};
assert.deepEqual(codes.slice().sort(), Object.keys(classification).sort());
const limits = {limitId:"codex",primary:{usedPercent:100,resetsAt:1000},secondary:{usedPercent:100,resetsAt:2000}};
for (const code of codes) {
  const info: JsonValue = code === "activeTurnNotSteerable" ? {[code]:{turnKind:"review"}} : variants[0].enum?.includes(code) ? code : {[code]:{httpStatusCode:null}};
  for (const message of ["opaque diagnostic", "Selected model is at capacity. Please try a different model."]) {
    const result = signal(info, limits, message);
    assert.equal(result.retryableKind, classification[code] === "transient" ? "transient" : classification[code] === "rate" ? "rate_limited_until" : undefined, code);
    assert.equal(result.retryAtMs, classification[code] === "rate" ? 2_000_000 : undefined, code);
    assert.equal(result.retryable, false);
  }
}
for (const code of ["httpConnectionFailed","responseStreamConnectionFailed","responseStreamDisconnected","responseTooManyFailedAttempts"]) {
  for (const status of [400,401,403,404,408,429,500,503,504]) {
    const result = signal({[code]:{httpStatusCode:status}},limits);
    assert.equal(result.retryableKind, status === 429 ? "rate_limited_until" : status === 408 || status >= 500 ? "transient" : undefined);
  }
}
for (const code of ["rateLimitExceeded","usageLimitExceeded"]) {
  for (const snapshot of [undefined, {}, {primary:{usedPercent:99,resetsAt:1000}}, {primary:{usedPercent:100}}, {primary:{usedPercent:100,resetsAt:-1}}, {limitId:"foreign",primary:{usedPercent:100,resetsAt:1000}}, {primary:{usedPercent:100},secondary:{usedPercent:100,resetsAt:1000}}]) {
    assert.equal(signal(code,snapshot).retryableKind,undefined);
  }
  assert.equal(signal(code,{primary:{usedPercent:100,resetsAt:1000},secondary:{usedPercent:20}}).retryAtMs,1_000_000);
}
for (const info of ["unknownFutureCode", {unknownFutureCode:{httpStatusCode:503}}, "cancelled", "unsupportedModel", {serverOverloaded:{},unauthorized:{}}, {httpConnectionFailed:{httpStatusCode:503},other:{}}]) {
  assert.equal(signal(info,limits).retryableKind,undefined);
}
for (const willRetry of [true, false]) {
  const result = runtime.runtimeReduce(context, null, [{ kind: "notification", method: "error", params: { threadId: "own", turnId: "turn", willRetry, error: { message: "opaque", codexErrorInfo: "serverOverloaded" } } }]);
  const event = result.events[0];
  if (event.kind !== "chatItem") throw new Error("Expected error signal");
  assert.equal((event.item.toolInput as Record<string, unknown>).retryable, willRetry);
  assert.equal(event.item.systemSubtype, willRetry ? "provider_retry" : "api_error");
}
const noMessage = runtime.runtimeReduce(context, null, [{ kind: "notification", method: "turn/completed", params: { threadId: "own", turn: { id: "turn", status: "failed", error: { codexErrorInfo: "serverOverloaded" } } } }]);
assert.equal(noMessage.events[0]?.kind, "chatItem");
const foreign = runtime.runtimeReduce(context, null, [{ kind: "notification", method: "turn/completed", params: { threadId: "foreign", turn: { id: "turn", status: "failed", error: { message: "opaque", codexErrorInfo: "serverOverloaded" } } } }]);
assert.equal(foreign.events.length, 0);
for (const method of ["account/updated", "account/login/completed"]) {
  const result = runtime.runtimeReduce(context,null,[
    {kind:"notification",method:"account/rateLimits/updated",params:{rateLimits:limits}},
    {kind:"notification",method,params:{authMode:"chatgpt",success:true,loginId:"login"}},
    {kind:"notification",method:"turn/completed",params:{threadId:"own",turn:{id:"turn",status:"failed",error:{message:"opaque",codexErrorInfo:"rateLimitExceeded"}}}},
  ]);
  const error = result.events.find(event => event.kind === "chatItem");
  if (error?.kind !== "chatItem") throw new Error("Expected error");
  assert.equal((error.item.toolInput as RuntimeErrorSignal).retryableKind,undefined);
  checks++;
}
console.log(`runtime retry signals: ${checks + 4} fixture checks passed; all ${codes.length} named schema variants classified`);
