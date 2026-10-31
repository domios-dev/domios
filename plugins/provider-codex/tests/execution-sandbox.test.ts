import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { AgentRuntime, ExecutionSandboxCapability, JsonValue, Operation, RuntimeContext, RuntimePlan, RuntimeSettings } from "@codeterm/plugin-sdk";

const manifest = JSON.parse(fs.readFileSync(new URL("../plugin.json", import.meta.url), "utf8"));
const capability = manifest.spawn.executionSandbox as ExecutionSandboxCapability;
const sandbox = vm.createContext({ module: { exports: {} }, exports: {}, host: new Proxy({}, { get: () => { throw new Error("Runtime sandbox mapping accessed host"); } }) });
sandbox.exports = sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), sandbox);
const provider = sandbox.module.exports.default as AgentRuntime;
const context: RuntimeContext = { epoch: 1, tabId: "opaque-tab", bindingNonce: "opaque-nonce", threadId: "opaque-thread", cwd: "/lane", hostVersion: "1.14.9", settings: {}, activeTurns: [], threadParents: [], claimedSends: [], providerSettings: {} };
const plain = <T>(value: T): T => JSON.parse(JSON.stringify(value));
function planned(ctx: RuntimeContext, operation: Operation): RuntimePlan {
  let plan = plain(provider.runtimePlan(ctx,operation));
  if (plan.action.kind === "request" && plan.action.call.method === "config/read") {
    for (const value of [{config:{}},{requirements:null}] as JsonValue[]) {
      const step = plain(provider.runtimeResponse(ctx,operation,plan.state,{kind:"success",value}));
      if (step.kind !== "continue") throw new Error("Sandbox selection did not continue");
      plan = step.plan;
    }
  }
  return plan;
}
for (const [mode, type] of [[capability.readOnly!, "readOnly"], [capability.mutating!, "workspaceWrite"]] as const) {
  const settings: RuntimeSettings = { mode: mode.runtimeMode!, additionalDirectories: [], networkAccess: false };
  for (const operation of [
    { kind: "openThread" as const, instructions: "opaque", settings },
    { kind: "resumeThread" as const, threadId: "opaque-thread", instructions: "opaque", settings },
    { kind: "submitTurn" as const, threadId: "opaque-thread", sendId: "opaque-send", input: { text: "opaque", attachments: [] }, settings },
    { kind: "control" as const, threadId: "opaque-thread", action: "settings", value: { mode: mode.runtimeMode!, additionalDirectories: [], networkAccess: false } },
  ]) {
    const plan = planned({ ...context, settings }, operation);
    assert.equal(plan.action.kind, "request");
    if (plan.action.kind !== "request") throw new Error("Missing request");
    const params = plan.action.call.params as Record<string, unknown>;
    assert.equal(params.approvalPolicy, "never");
    if (operation.kind === "openThread" || operation.kind === "resumeThread") {
      assert.equal(params.sandbox, type === "readOnly" ? "read-only" : "workspace-write");
      const config = params.config as Record<string, unknown>;
      assert.deepEqual(config["sandbox_workspace_write.writable_roots"], []);
      assert.equal(config["sandbox_workspace_write.exclude_tmpdir_env_var"], true);
      assert.equal(config["sandbox_workspace_write.exclude_slash_tmp"], true);
    } else {
      const policy = params.sandboxPolicy as Record<string, unknown>;
      assert.equal(policy.type, type);
    }
  }
  assert(mode.launchArgs.includes("{cwd}"));
  assert(mode.launchArgs.includes("never"));
  assert(!mode.launchArgs.some(arg => arg.includes("dangerously")));
}
const roots: RuntimeSettings = { mode: "workspace-write", additionalDirectories: ["/lane-pad", "/git-metadata"], networkAccess: false };
const plan = plain(provider.runtimePlan({ ...context, settings: roots }, { kind: "submitTurn", threadId: "opaque-thread", sendId: "opaque-send", input: { text: "opaque", attachments: [] }, settings: roots }));
if (plan.action.kind !== "request") throw new Error("Missing request");
assert.deepEqual((plan.action.call.params as { sandboxPolicy: { writableRoots: string[] } }).sandboxPolicy.writableRoots, roots.additionalDirectories);
for (const [mode, type, approval] of [["full-access", "dangerFullAccess", "never"], ["auto", "workspaceWrite", "on-request"]] as const) {
  const settings: RuntimeSettings = {mode};
  const result = plain(provider.runtimePlan({...context, settings}, {kind:"submitTurn", threadId:"opaque-thread", sendId:"opaque-send", input:{text:"opaque", attachments:[]}, settings}));
  if (result.action.kind !== "request") throw new Error("Missing request");
  const params = result.action.call.params as {approvalPolicy:string; sandboxPolicy:{type:string}};
  assert.equal(params.approvalPolicy, approval);
  assert.equal(params.sandboxPolicy.type, type);
}

const fullSettings: RuntimeSettings = {mode:"full-access",additionalDirectories:["/own-pad"]};
for (const operation of [
  {kind:"openThread" as const,instructions:"opaque",settings:fullSettings},
  {kind:"resumeThread" as const,threadId:"opaque-thread",instructions:"opaque",settings:fullSettings},
  {kind:"submitTurn" as const,threadId:"opaque-thread",sendId:"opaque-send",input:{text:"opaque",attachments:[]},settings:fullSettings},
]) {
  const result=plain(provider.runtimePlan({...context,settings:fullSettings},operation));
  if (result.action.kind !== "request") throw new Error("Missing full-access request");
  const params=result.action.call.params as Record<string,unknown>;
  assert.equal(params.approvalPolicy,"never");
  if (operation.kind === "submitTurn") {
    assert.equal((params.sandboxPolicy as Record<string,unknown>).type,"dangerFullAccess");
  } else {
    assert.equal(params.sandbox,"danger-full-access");
    assert.equal((params.config as Record<string,unknown>)["windows.sandbox"],undefined);
  }
}

const padSettings: RuntimeSettings = {mode:capability.runtimeReadOnlyWritableRootsMode!,additionalDirectories:["/own-pad"],networkAccess:false};
assert.equal(padSettings.mode,"read-only-with-writable-roots");
for (const operation of [
  {kind:"openThread" as const,instructions:"opaque",settings:padSettings},
  {kind:"resumeThread" as const,threadId:"opaque-thread",instructions:"opaque",settings:padSettings},
  {kind:"submitTurn" as const,threadId:"opaque-thread",sendId:"opaque-send",input:{text:"opaque",attachments:[]},settings:padSettings},
]) {
  const result=plain(provider.runtimePlan({...context,settings:padSettings},operation));
  if (result.action.kind !== "request") throw new Error("Missing pad-write request");
  const params=result.action.call.params as Record<string,unknown>;
  assert.equal(params.approvalPolicy,"never");
  assert.equal(params.cwd,"/lane");
  assert.deepEqual(params.runtimeWorkspaceRoots,["/own-pad"]);
  if (operation.kind === "submitTurn") {
    assert.deepEqual(params.sandboxPolicy,{type:"workspaceWrite",networkAccess:false,writableRoots:["/own-pad"],excludeTmpdirEnvVar:true,excludeSlashTmp:true});
  } else {
    assert.equal(params.sandbox,"workspace-write");
    assert.deepEqual((params.config as Record<string,unknown>)["sandbox_workspace_write.writable_roots"],["/own-pad"]);
    const reply={thread:{id:"opaque-thread",source:"user"},approvalPolicy:"never",runtimeWorkspaceRoots:["/own-pad"],sandbox:{type:"workspaceWrite",writableRoots:["/own-pad"],excludeTmpdirEnvVar:true,excludeSlashTmp:true}};
    const accepted=plain(provider.runtimeResponse({...context,settings:padSettings},operation,result.state,{kind:"success",value:reply}));
    assert.notEqual(accepted.kind,"refused");
    for (const wrong of [{...reply,runtimeWorkspaceRoots:["/lane","/own-pad"]},{thread:reply.thread,approvalPolicy:reply.approvalPolicy,sandbox:reply.sandbox},{...reply,sandbox:{...reply.sandbox,excludeSlashTmp:false}},{...reply,sandbox:{...reply.sandbox,writableRoots:["/foreign-pad"]}},{...reply,sandbox:{type:"dangerFullAccess"}}]) {
      const denied=plain(provider.runtimeResponse({...context,settings:padSettings},operation,result.state,{kind:"success",value:wrong}));
      assert.equal(denied.kind,"refused");
    }
  }
}
const control=plain(provider.runtimePlan({...context,settings:padSettings},{kind:"control",threadId:"opaque-thread",action:"model",value:"opaque-model"}));
if (control.action.kind !== "request") throw new Error("Missing model control");
assert.equal(control.action.call.method,"thread/settings/update");
assert.equal((control.action.call.params as Record<string,unknown>).sandboxPolicy,undefined);
assert.equal((control.action.call.params as Record<string,unknown>).runtimeWorkspaceRoots,undefined);
assert.throws(()=>provider.runtimePlan({...context,settings:padSettings},{kind:"control",threadId:"opaque-thread",action:"settings",value:{additionalDirectories:["/other-pad"]}}));

const folderSettings: RuntimeSettings = {mode:"workspace-write",additionalDirectories:["/own-pad"],networkAccess:false};
for (const operation of [
  {kind:"openThread" as const,instructions:"opaque",settings:folderSettings},
  {kind:"resumeThread" as const,threadId:"opaque-thread",instructions:"opaque",settings:folderSettings},
  {kind:"submitTurn" as const,threadId:"opaque-thread",sendId:"opaque-folder-send",input:{text:"opaque",attachments:[]},settings:folderSettings},
]) {
  const result=planned({...context,settings:folderSettings},operation);
  if (result.action.kind!=="request") throw new Error("Missing folder request");
  const params=result.action.call.params as Record<string,unknown>;
  assert.equal(params.approvalPolicy,"never");
  if (operation.kind==="submitTurn") {
    assert.deepEqual(params.sandboxPolicy,{type:"workspaceWrite",networkAccess:false,writableRoots:["/own-pad"],excludeTmpdirEnvVar:true,excludeSlashTmp:true});
  } else {
    assert.equal(params.sandbox,"workspace-write");
    assert.equal(params.cwd,"/lane");
    assert.deepEqual((params.config as Record<string,unknown>)["sandbox_workspace_write.writable_roots"],["/own-pad"]);
    assert.equal((params.config as Record<string,unknown>)["windows.sandbox"],"mxc");
  }
}

for (const operation of [
  {kind:"openThread" as const,instructions:"opaque",settings:folderSettings},
  {kind:"resumeThread" as const,threadId:"opaque-thread",instructions:"opaque",settings:folderSettings},
]) {
  const ctx = {...context,settings:folderSettings};
  const initial = plain(provider.runtimePlan(ctx,operation));
  if (initial.action.kind !== "request") throw new Error("Missing sandbox config read");
  assert.equal(initial.action.call.method,"config/read");
  assert.deepEqual(initial.action.call.params,{includeLayers:false,cwd:"/lane"});
  const configured = plain(provider.runtimeResponse(ctx,operation,initial.state,{kind:"success",value:{config:{windows:{sandbox:"elevated"}}}}));
  if (configured.kind !== "continue" || configured.plan.action.kind !== "request") throw new Error("Configured backend did not continue");
  const expectedMethod = operation.kind === "openThread" ? "thread/start" : "thread/resume";
  assert.equal(configured.plan.action.call.method,expectedMethod);
  assert.equal((configured.plan.action.call.params as {config:Record<string,unknown>}).config["windows.sandbox"],undefined);
  const readingRequirements = plain(provider.runtimeResponse(ctx,operation,initial.state,{kind:"success",value:{config:{}}}));
  if (readingRequirements.kind !== "continue" || readingRequirements.plan.action.kind !== "request") throw new Error("Missing requirements read");
  assert.equal(readingRequirements.plan.action.call.method,"configRequirements/read");
  assert.equal(readingRequirements.plan.action.call.params,null);
  for (const stage of [initial.state,readingRequirements.plan.state]) {
    const failed = plain(provider.runtimeResponse(ctx,operation,stage,{kind:"error",error:{code:-32603,message:"opaque read failure"}}));
    assert.equal(failed.kind,"refused");
  }
  assert.throws(()=>provider.runtimeResponse(ctx,operation,initial.state,{kind:"success",value:{}}));
  assert.throws(()=>provider.runtimeResponse(ctx,operation,readingRequirements.plan.state,{kind:"success",value:{requirements:{}}}));
  const restricted = plain(provider.runtimeResponse(ctx,operation,readingRequirements.plan.state,{kind:"success",value:{requirements:{allowedWindowsSandboxImplementations:["elevated"],allowedSandboxModes:["workspace-write"]}}}));
  if (restricted.kind !== "continue" || restricted.plan.action.kind !== "request") throw new Error("Managed backend did not continue");
  assert.equal((restricted.plan.action.call.params as {config:Record<string,unknown>}).config["windows.sandbox"],undefined);
  const selected = planned(ctx,operation);
  const rawReply = {thread:{id:"opaque-thread"},approvalPolicy:"never",approvalsReviewer:"user",sandbox:{type:"workspaceWrite",networkAccess:false}};
  for (const reply of [
    {...rawReply,sandbox:{type:"readOnly"}},
    {...rawReply,mode:"workspace-write",sandbox:{type:"readOnly"}},
    {...rawReply,sandbox:{type:"dangerFullAccess"}},
    {...rawReply,sandbox:{type:"workspaceWrite",networkAccess:true}},
    {...rawReply,sandbox:{type:"workspaceWrite"}},
    {thread:rawReply.thread},
  ] as JsonValue[]) {
    assert.equal(plain(provider.runtimeResponse(ctx,operation,selected.state,{kind:"success",value:reply})).kind,"refused");
  }
  assert.notEqual(plain(provider.runtimeResponse(ctx,operation,selected.state,{kind:"success",value:rawReply})).kind,"refused");
  if (operation.kind === "resumeThread") {
    const unarchive = plain(provider.runtimeResponse(ctx,operation,selected.state,{kind:"error",error:{code:-32600,message:"session opaque-thread is archived"}}));
    if (unarchive.kind !== "continue") throw new Error("Missing unarchive continuation");
    const resumed = plain(provider.runtimeResponse(ctx,operation,unarchive.plan.state,{kind:"success",value:{}}));
    if (resumed.kind !== "continue" || resumed.plan.action.kind !== "request") throw new Error("Missing resumed request");
    assert.equal((resumed.plan.action.call.params as {config:Record<string,unknown>}).config["windows.sandbox"],"mxc");
  }
}
console.log("Codex execution sandbox mappings and session-local backend selection passed");
