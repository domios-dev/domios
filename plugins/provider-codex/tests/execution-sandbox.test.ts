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
    for (const [method,value] of [["config/read",{config:{}}],["configRequirements/read",{requirements:null}]] as [string,JsonValue][]) {
      if (plan.action.kind !== "request" || plan.action.call.method !== method) break;
      const step = plain(provider.runtimeResponse(ctx,operation,plan.state,{kind:"success",value}));
      if (step.kind !== "continue") throw new Error("Sandbox selection did not continue");
      plan = step.plan;
    }
  }
  return plan;
}
for (const [mode, splitWindowsRoots] of [
  ["workspace-write",undefined],
  ["read-only-with-writable-roots",undefined],
  ["read-only-with-writable-roots",true],
] as const) {
  const cwd = "C:/Users/fixture/existing-git/friend";
  const settings: RuntimeSettings = {mode,networkAccess:false,additionalDirectories:["C:/own-pad"]};
  const restrictedCli = {
    tabId:context.tabId,bindingNonce:context.bindingNonce,epoch:context.epoch,cwd,metadataDirectory:".codex",
    executable:"C:/trusted/cli.exe",executableSha256:"a".repeat(64),configSha256:"b".repeat(64),ruleSha256:"c".repeat(64),
    ...(splitWindowsRoots ? {splitWindowsRoots:true} : {}),
  };
  const ctx: RuntimeContext = {...context,threadId:"pre-existing-session",cwd,settings,restrictedCli};
  const operation: Operation = {kind:"resumeThread",threadId:"pre-existing-session",instructions:"opaque",settings};
  const plan = planned(ctx,operation);
  if (plan.action.kind !== "request") throw new Error("Missing resume request");
  assert.equal(plan.action.call.method,"thread/resume");
  const params = plan.action.call.params as Record<string, unknown>;
  assert.equal(params.threadId,"pre-existing-session");
  assert.equal(params.cwd,cwd);
  const config = params.config as Record<string,unknown>;
  assert.deepEqual(config.project_root_markers,[]);
  assert.deepEqual(config.projects,{[cwd]:{trust_level:"trusted"}});
  assert.deepEqual(config["sandbox_workspace_write.writable_roots"],settings.additionalDirectories);
  assert.equal(config["sandbox_workspace_write.network_access"],false);
  assert.equal(config.CODEX_HOME,undefined);

  const initial = plain(provider.runtimePlan(ctx,operation));
  if (!splitWindowsRoots) {
    assert.equal(config["windows.sandbox"],undefined);
    for (const windows of [{},{sandbox:"unelevated",allow_mxc:null}]) {
      const inherited = plain(provider.runtimeResponse(ctx,operation,initial.state,{kind:"success",value:{config:{windows}}}));
      if (inherited.kind !== "continue" || inherited.plan.action.kind !== "request") throw new Error("Missing inherited backend request");
      assert.equal(inherited.plan.action.call.method,"thread/resume");
      assert.deepEqual(inherited.plan.action.call.params,params);
    }
    continue;
  }
  const unelevated = plain(provider.runtimeResponse(ctx,operation,initial.state,{kind:"success",value:{config:{windows:{sandbox:"unelevated",allow_mxc:null}}}}));
  if (unelevated.kind !== "continue" || unelevated.plan.action.kind !== "request") throw new Error("Missing sandbox continuation");
  assert.equal(unelevated.plan.action.call.method,"configRequirements/read");
  for (const [requirements, backend] of [
    [null,"mxc"],
    [{allowedWindowsSandboxImplementations:["unelevated"],allowedSandboxModes:["workspace-write"]},undefined],
  ] as const) {
    const reply = {kind:"success" as const,value:{requirements:JSON.parse(JSON.stringify(requirements))}};
    const next = plain(provider.runtimeResponse(ctx,operation,unelevated.plan.state,reply));
    if (next.kind !== "continue" || next.plan.action.kind !== "request") throw new Error("Missing checked resume");
    assert.equal(next.plan.action.call.method,"thread/resume");
    const selected = next.plan.action.call.params as {config:Record<string,unknown>;runtimeWorkspaceRoots:string[]};
    assert.equal(selected.config["windows.sandbox"],backend);
    assert.deepEqual(selected.runtimeWorkspaceRoots,["C:/own-pad"]);
    assert.deepEqual(selected.config["sandbox_workspace_write.writable_roots"],["C:/own-pad"]);
    assert.equal(selected.config["sandbox_workspace_write.network_access"],false);
  }
}

const managedSettings: RuntimeSettings = {mode:"read-only-with-writable-roots",networkAccess:false,additionalDirectories:["C:/own-pad"]};
const managedCwd = "C:/fixture/friend";
const managedPin = {tabId:context.tabId,bindingNonce:context.bindingNonce,epoch:context.epoch,cwd:managedCwd,metadataDirectory:".codex",
  executable:"C:/trusted/cli.exe",executableSha256:"a".repeat(64),configSha256:"b".repeat(64),ruleSha256:"c".repeat(64),splitWindowsRoots:true};
const managedCtx = {...context,cwd:managedCwd,settings:managedSettings,restrictedCli:managedPin};
const managedErrorPrefix = "failed to load configuration: invalid value for `windows.sandbox`: `Some(Mxc)` is not in the allowed set windows.allow_mxc = false (set by ";
const managedError = {kind:"error" as const,error:{code:-32600,message:managedErrorPrefix+"C:/Program Files (x86)/fixture/requirements.toml)"}};
for (const operation of [
  {kind:"openThread" as const,settings:managedSettings},
  {kind:"resumeThread" as const,threadId:"opaque-thread",settings:managedSettings},
]) {
  const initial = plain(provider.runtimePlan(managedCtx,operation));
  const requirements = plain(provider.runtimeResponse(managedCtx,operation,initial.state,{kind:"success",value:{config:{windows:{sandbox:"unelevated",allow_mxc:null}}}}));
  if (requirements.kind !== "continue") throw new Error("Missing managed requirements request");
  const selected = plain(provider.runtimeResponse(managedCtx,operation,requirements.plan.state,{kind:"success",value:{requirements:null}}));
  if (selected.kind !== "continue" || selected.plan.action.kind !== "request") throw new Error("Missing MXC request");
  const {config: override, ...params} = selected.plan.action.call.params as {config:Record<string,unknown>};
  const {"windows.sandbox": backend,...inherited} = override;
  assert.equal(backend,"mxc");
  const fallback = plain(provider.runtimeResponse(managedCtx,operation,selected.plan.state,managedError));
  if (fallback.kind !== "continue" || fallback.plan.action.kind !== "request") throw new Error("Missing inherited backend request");
  assert.equal(fallback.plan.action.call.method,selected.plan.action.call.method);
  assert.deepEqual(fallback.plan.action.call.params,{...params,config:inherited});
  assert.equal(plain(provider.runtimeResponse(managedCtx,operation,fallback.plan.state,managedError)).kind,"refused");
  for (const message of [
    "unrelated refusal",
    managedErrorPrefix,
    managedErrorPrefix+")",
    managedErrorPrefix+"C:/fixture/requirements.toml",
    managedErrorPrefix+"x".repeat(4097)+")",
    managedError.error.message+": extra diagnostic (details)",
    managedError.error.message+"\n",
  ]) {
    const rejected = plain(provider.runtimeResponse(managedCtx,operation,selected.plan.state,{kind:"error",error:{code:-32600,message}}));
    assert.equal(rejected.kind,"refused");
  }
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
  const result=planned({...context,settings:padSettings},operation);
  if (result.action.kind !== "request") throw new Error("Missing pad-write request");
  const params=result.action.call.params as Record<string,unknown>;
  assert.equal(params.approvalPolicy,"never");
  assert.equal(params.cwd,"/lane");
  assert.deepEqual(params.runtimeWorkspaceRoots,["/own-pad"]);
  if (operation.kind === "submitTurn") {
    assert.deepEqual(params.sandboxPolicy,{type:"workspaceWrite",networkAccess:false,writableRoots:["/own-pad"],excludeTmpdirEnvVar:true,excludeSlashTmp:true});
  } else {
    assert.equal(params.sandbox,"workspace-write");
    assert.equal((params.config as Record<string,unknown>)["windows.sandbox"],"mxc");
    assert.deepEqual((params.config as Record<string,unknown>)["sandbox_workspace_write.writable_roots"],["/own-pad"]);
    const reply={thread:{id:"opaque-thread",source:"user"},approvalPolicy:"never",runtimeWorkspaceRoots:["/own-pad"],sandbox:{type:"workspaceWrite",networkAccess:false,writableRoots:["/own-pad"],excludeTmpdirEnvVar:true,excludeSlashTmp:true}};
    const accepted=plain(provider.runtimeResponse({...context,settings:padSettings},operation,result.state,{kind:"success",value:reply}));
    assert.notEqual(accepted.kind,"refused");
    for (const wrong of [{...reply,runtimeWorkspaceRoots:["/lane","/own-pad"]},{thread:reply.thread,approvalPolicy:reply.approvalPolicy,sandbox:reply.sandbox},{...reply,sandbox:{...reply.sandbox,networkAccess:true}},{...reply,sandbox:{...reply.sandbox,excludeSlashTmp:false}},{...reply,sandbox:{...reply.sandbox,writableRoots:["/foreign-pad"]}},{...reply,sandbox:{type:"dangerFullAccess"}}]) {
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
for (const [mode, cwd, pad, command] of [
  ["read-only-with-writable-roots", "C:/opaque-read-root", "C:/opaque-pad", "C:/host/current CLI.exe"],
  ["read-only-with-writable-roots", "/opaque-read-root", "/opaque-pad", "/private/tmp/host/current CLI"],
  ["workspace-write", "/opaque-folder-root", "/opaque-pad", "/private/tmp/host/current CLI"],
]) {
  const settings: RuntimeSettings = {mode,networkAccess:false,additionalDirectories:[pad]};
  const server = {name:"domios_neighbor_reply",command,args:["mcp","serve","--tab",context.tabId,"--binding-nonce",context.bindingNonce],
    env:{CODETERM_SOCKET_PATH:"opaque instance",CODETERM_NEIGHBOR_REPLY_FRIENDSHIP_ID:"a".repeat(64)}};
  const ctx = {...context,cwd,settings,hostMcp:server};
  for (const operation of [
    {kind:"openThread",instructions:"opaque guidance",settings},
    {kind:"resumeThread",threadId:context.threadId!,instructions:"opaque guidance",settings},
  ] satisfies Operation[]) {
    const plan = planned(ctx,operation);
    if (plan.action.kind !== "request") throw new Error("Missing MCP-bound thread request");
    assert.equal(plan.action.call.method,operation.kind === "openThread" ? "thread/start" : "thread/resume");
    const params = plan.action.call.params as {config:Record<string,unknown>;runtimeWorkspaceRoots?:string[];developerInstructions:string;approvalPolicy:string};
    assert.deepEqual(params.config["mcp_servers.domios_neighbor_reply"],{
      command,args:server.args,env:server.env,env_vars:["CODETERM_API_TOKEN"],required:true,
      enabled_tools:["reply"],tools:{reply:{approval_mode:"approve"}},
    });
    assert.equal(params.config["sandbox_workspace_write.network_access"],false);
    assert.equal(params.approvalPolicy,"never");
    assert.deepEqual(params.runtimeWorkspaceRoots,mode === "read-only-with-writable-roots" ? [pad] : undefined);
    assert.deepEqual(params.config["sandbox_workspace_write.writable_roots"],[pad]);
    assert.ok(params.developerInstructions.startsWith("opaque guidance\n\n"));
    assert.ok(params.developerInstructions.includes(JSON.stringify(cwd)));
    assert.equal(params.config.projects,undefined);
  }
}
console.log("Codex execution sandbox mappings and session-local backend selection passed");
