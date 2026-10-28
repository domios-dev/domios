import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { AgentRuntime, ExecutionSandboxCapability, RuntimeContext, RuntimeSettings } from "@codeterm/plugin-sdk";

const manifest = JSON.parse(fs.readFileSync(new URL("../plugin.json", import.meta.url), "utf8"));
const capability = manifest.spawn.executionSandbox as ExecutionSandboxCapability;
const sandbox = vm.createContext({ module: { exports: {} }, exports: {}, host: new Proxy({}, { get: () => { throw new Error("Runtime sandbox mapping accessed host"); } }) });
sandbox.exports = sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), sandbox);
const provider = sandbox.module.exports.default as AgentRuntime;
const context: RuntimeContext = { epoch: 1, tabId: "opaque-tab", bindingNonce: "opaque-nonce", threadId: "opaque-thread", cwd: "/lane", hostVersion: "1.14.9", settings: {}, activeTurns: [], threadParents: [], claimedSends: [], providerSettings: {} };
const plain = <T>(value: T): T => JSON.parse(JSON.stringify(value));
for (const [mode, type] of [[capability.readOnly!, "readOnly"], [capability.mutating!, "workspaceWrite"]] as const) {
  const settings: RuntimeSettings = { mode: mode.runtimeMode!, additionalDirectories: [], networkAccess: false };
  for (const operation of [
    { kind: "openThread" as const, instructions: "opaque", settings },
    { kind: "resumeThread" as const, threadId: "opaque-thread", instructions: "opaque", settings },
    { kind: "submitTurn" as const, threadId: "opaque-thread", sendId: "opaque-send", input: { text: "opaque", attachments: [] }, settings },
    { kind: "control" as const, threadId: "opaque-thread", action: "settings", value: { mode: mode.runtimeMode!, additionalDirectories: [], networkAccess: false } },
  ]) {
    const plan = plain(provider.runtimePlan({ ...context, settings }, operation));
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
console.log("Codex execution sandbox: 11 operation mappings passed");

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
  const result=plain(provider.runtimePlan({...context,settings:folderSettings},operation));
  if (result.action.kind!=="request") throw new Error("Missing folder request");
  const params=result.action.call.params as Record<string,unknown>;
  assert.equal(params.approvalPolicy,"never");
  if (operation.kind==="submitTurn") {
    assert.deepEqual(params.sandboxPolicy,{type:"workspaceWrite",networkAccess:false,writableRoots:["/own-pad"],excludeTmpdirEnvVar:true,excludeSlashTmp:true});
  } else {
    assert.equal(params.sandbox,"workspace-write");
    assert.equal(params.cwd,"/lane");
    assert.deepEqual((params.config as Record<string,unknown>)["sandbox_workspace_write.writable_roots"],["/own-pad"]);
  }
}
