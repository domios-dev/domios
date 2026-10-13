import type { JsonValue, RuntimeSettings } from "@codeterm/plugin-sdk";
import { object, text } from "./runtimeProtocol";

export function codexRuntimePolicy(settings: RuntimeSettings): Record<string, unknown> {
  const selected = settings.mode ?? "auto-accept-edits";
  const policies: Record<string, [string,string,string]> = {
    "approval-required":["untrusted","user","readOnly"],
    "auto-accept-edits":["on-request","user","workspaceWrite"],
    "auto":["on-request","auto_review","workspaceWrite"],
    "full-access":["never","user","dangerFullAccess"],
  };
  const policy = policies[selected];
  if (!policy) throw {code:"unsupported",message:`Unsupported runtime mode: ${selected}`};
  return {approvalPolicy:policy[0],approvalsReviewer:policy[1],sandboxPolicy:{type:policy[2],
    ...(policy[2] === "workspaceWrite" && settings.networkAccess !== undefined ? {networkAccess:settings.networkAccess} : {}),
    ...(policy[2] === "workspaceWrite" && settings.additionalDirectories?.length ? {writableRoots:[...settings.additionalDirectories]} : {})}};
}
export function codexThreadConfig(settings: RuntimeSettings): Record<string, JsonValue> {
  const workspaceWrite = (codexRuntimePolicy(settings).sandboxPolicy as {type:string}).type === "workspaceWrite";
  return {check_for_update_on_startup:false,project_doc_max_bytes:65536,
    ...(workspaceWrite && settings.networkAccess !== undefined ? {"sandbox_workspace_write.network_access":settings.networkAccess} : {}),
    ...(settings.additionalDirectories?.length ? {"sandbox_workspace_write.writable_roots":[...settings.additionalDirectories]} : {}),
    ...(settings.effort ? {model_reasoning_effort:settings.effort} : {})};
}

export function codexReportedSettings(raw: unknown): RuntimeSettings {
  const root = object(raw);
  const settings = root.threadSettings ? object(root.threadSettings) : root;
  const thread = object(settings.thread);
  const model = text(settings.model) ?? text(thread.model);
  const effort = text(settings.reasoningEffort) ?? text(settings.effort) ?? text(thread.reasoningEffort) ?? text(thread.effort);
  const sandbox = object(settings.sandboxPolicy ?? settings.sandbox ?? thread.sandboxPolicy ?? thread.sandbox).type;
  const approval = settings.approvalPolicy ?? thread.approvalPolicy;
  const reviewer = settings.approvalsReviewer ?? thread.approvalsReviewer;
  const mode = text(settings.mode) ?? text(thread.mode) ?? (approval === "untrusted" ? "approval-required"
    : approval === "never" && sandbox === "dangerFullAccess" ? "full-access"
    : approval === "on-request" && reviewer === "auto_review" ? "auto"
    : approval === "on-request" && reviewer === "user" && sandbox === "workspaceWrite" ? "auto-accept-edits" : undefined);
  return {...(model ? {model} : {}),...(effort ? {effort} : {}),...(mode ? {mode} : {})};
}
export function codexEffectiveSettings(requested: RuntimeSettings, reply: unknown): RuntimeSettings {
  return {...requested,...codexReportedSettings(reply)};
}
