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
