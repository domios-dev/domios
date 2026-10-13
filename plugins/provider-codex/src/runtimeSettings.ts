import type { JsonValue, RuntimeSettings } from "@codeterm/plugin-sdk";

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
