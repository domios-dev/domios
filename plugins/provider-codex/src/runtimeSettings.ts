import type { HostOwnedCapabilities, JsonValue, RuntimeSettings } from "@codeterm/plugin-sdk";
import { object, text } from "./runtimeProtocol";

export function codexServiceTier(settings: RuntimeSettings): Record<string, string | null> {
  return settings.serviceTier === undefined ? {} : {serviceTier:settings.serviceTier.id};
}

export function codexRuntimePolicy(settings: RuntimeSettings): Record<string, unknown> {
  const selected = settings.mode ?? "auto-accept-edits";
  const policies: Record<string, [string,string,string]> = {
    "approval-required":["untrusted","user","readOnly"],
    "auto-accept-edits":["on-request","user","workspaceWrite"],
    "auto":["on-request","auto_review","workspaceWrite"],
    "full-access":["never","user","dangerFullAccess"],
    "read-only":["never","user","readOnly"],
    "read-only-with-writable-roots":["never","user","workspaceWrite"],
    "workspace-write":["never","user","workspaceWrite"],
  };
  const policy = policies[selected];
  if (!policy) throw {code:"unsupported",message:`Unsupported runtime mode: ${selected}`};
  const padOnly = selected === "read-only-with-writable-roots";
  if (padOnly && !settings.additionalDirectories?.length) throw {code:"invalid_schema",message:"Read-only writable roots are missing"};
  return {approvalPolicy:policy[0],approvalsReviewer:policy[1],...(padOnly ? {runtimeWorkspaceRoots:[...settings.additionalDirectories!]} : {}),sandboxPolicy:{type:policy[2],
    ...((policy[2] === "workspaceWrite" || selected === "read-only") && settings.networkAccess !== undefined ? {networkAccess:settings.networkAccess} : {}),
    ...(selected === "workspace-write" || padOnly ? {writableRoots:[...(settings.additionalDirectories ?? [])],excludeTmpdirEnvVar:true,excludeSlashTmp:true} : {}),
    ...(policy[2] === "workspaceWrite" && settings.additionalDirectories?.length ? {writableRoots:[...settings.additionalDirectories]} : {})}};
}
export function codexThreadConfig(settings: RuntimeSettings, hostOwned?: HostOwnedCapabilities): Record<string, JsonValue> {
  const workspaceWrite = (codexRuntimePolicy(settings).sandboxPolicy as {type:string}).type === "workspaceWrite";
  // Codex has no feature switch for update_plan.
  return {check_for_update_on_startup:false,project_doc_max_bytes:65536,
    ...(["read-only","workspace-write","read-only-with-writable-roots"].includes(settings.mode ?? "") ? {
      "sandbox_workspace_write.writable_roots":[...(settings.additionalDirectories ?? [])],
      "sandbox_workspace_write.exclude_tmpdir_env_var":true,"sandbox_workspace_write.exclude_slash_tmp":true,
    } : {}),
    ...(workspaceWrite && settings.networkAccess !== undefined ? {"sandbox_workspace_write.network_access":settings.networkAccess} : {}),
    ...(settings.additionalDirectories?.length ? {"sandbox_workspace_write.writable_roots":[...settings.additionalDirectories]} : {}),
    ...(settings.effort ? {model_reasoning_effort:settings.effort} : {}),
    ...(hostOwned?.questions ? {"features.default_mode_request_user_input":false} : {})};
}

export function codexReportedSettings(raw: unknown, requested?: RuntimeSettings): RuntimeSettings {
  const root = object(raw);
  const settings = root.threadSettings ? object(root.threadSettings) : root;
  const thread = object(settings.thread);
  const tier = settings.serviceTier !== undefined ? settings.serviceTier : thread.serviceTier;
  const serviceTier = tier === undefined ? undefined : tier === null || typeof tier === "string" ? {id:tier === "default" ? null : tier === "fast" ? "priority" : tier} : undefined;
  const model = text(settings.model) ?? text(thread.model);
  const effort = text(settings.reasoningEffort) ?? text(settings.effort) ?? text(thread.reasoningEffort) ?? text(thread.effort);
  const sandbox = object(settings.sandboxPolicy ?? settings.sandbox ?? thread.sandboxPolicy ?? thread.sandbox).type;
  const approval = settings.approvalPolicy ?? thread.approvalPolicy;
  const reviewer = settings.approvalsReviewer ?? thread.approvalsReviewer;
  const mode = text(settings.mode) ?? text(thread.mode) ?? (approval === "untrusted" ? "approval-required"
    : approval === "never" && sandbox === "dangerFullAccess" ? "full-access"
    : approval === "never" && sandbox === "readOnly" ? "read-only"
    : approval === "never" && sandbox === "workspaceWrite" ? "workspace-write"
    : approval === "on-request" && reviewer === "auto_review" ? "auto"
    : approval === "on-request" && reviewer === "user" && sandbox === "workspaceWrite" ? "auto-accept-edits" : undefined);
  const effectiveMode = requested?.mode === "read-only-with-writable-roots" && mode === "workspace-write" ? requested.mode : mode;
  return {...(serviceTier !== undefined ? {serviceTier} : {}),...(model ? {model} : {}),...(effort ? {effort} : {}),...(effectiveMode ? {mode:effectiveMode} : {})};
}
export function codexEffectiveSettings(requested: RuntimeSettings, reply: unknown): RuntimeSettings {
  const {serviceTier: _requestedTier,...other} = requested;
  return {...other,...codexReportedSettings(reply,requested)};
}

export function codexWritableRootsConfirmed(settings: RuntimeSettings, reply: unknown): boolean {
  if (settings.mode !== "read-only-with-writable-roots") return true;
  const root = object(reply);
  const reported = root.threadSettings ? object(root.threadSettings) : root;
  const roots = reported.runtimeWorkspaceRoots;
  const sandbox = object(reported.sandboxPolicy ?? reported.sandbox);
  const expected = settings.additionalDirectories ?? [];
  const same = (value: unknown) => Array.isArray(value) && value.length === expected.length && value.every(path => typeof path === "string" && expected.includes(path));
  return expected.length > 0 && reported.approvalPolicy === "never" && sandbox.type === "workspaceWrite"
    && sandbox.excludeTmpdirEnvVar === true && sandbox.excludeSlashTmp === true && same(roots) && same(sandbox.writableRoots);
}
