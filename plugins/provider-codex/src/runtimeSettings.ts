import type { HostOwnedCapabilities, JsonValue, RuntimeSettings, RuntimeContext } from "@codeterm/plugin-sdk";
import { object, text } from "./runtimeProtocol";

const windowsSandboxFeatures = ["prefer_mxc", "experimental_windows_sandbox", "enable_experimental_windows_sandbox", "elevated_windows_sandbox"];

export function codexRestrictedCliConfig(ctx: RuntimeContext): Record<string, JsonValue> {
  const policy = ctx.restrictedCli;
  if (!policy) return {};
  const hash = (value: string) => /^[0-9a-f]{64}$/.test(value);
  if (policy.tabId !== ctx.tabId || policy.bindingNonce !== ctx.bindingNonce || policy.epoch !== ctx.epoch
    || policy.cwd !== ctx.cwd || policy.metadataDirectory !== ".codex" || ctx.settings.mode !== "workspace-write" || ctx.settings.networkAccess !== false
    || !/^[A-Za-z0-9/\\:._-]+$/.test(policy.executable)
    || !/^(?:[A-Za-z]:[\\/]|\/)/.test(policy.executable) || policy.executable.includes("\0")
    || ![policy.executableSha256,policy.configSha256,policy.ruleSha256].every(hash))
    throw {code:"binding_mismatch",message:"Restricted CLI policy differs from the bound launch"};
  return {projects:{[ctx.cwd]:{trust_level:"trusted"}}};
}

export function codexValidateCliTrust(ctx: RuntimeContext, raw: unknown): void {
  if (!ctx.restrictedCli) return;
  codexRestrictedCliConfig(ctx);
  const root = object(raw);
  if (!root.config || typeof root.config !== "object" || Array.isArray(root.config))
    throw {code:"invalid_schema",message:"Restricted CLI project trust was not reported"};
  const config = object(root.config);
  if (config.projects === undefined || config.projects === null) return;
  if (typeof config.projects !== "object" || Array.isArray(config.projects))
    throw {code:"invalid_schema",message:"Invalid configured project trust"};
  const normalize = (path: string) => path.replace(/\\/g,"/").replace(/\/$/,"").toLowerCase();
  const cwd = normalize(ctx.cwd);
  for (const [path,value] of Object.entries(object(config.projects))) {
    if (cwd !== normalize(path) && !cwd.startsWith(normalize(path)+"/")) continue;
    if (!value || typeof value !== "object" || Array.isArray(value))
      throw {code:"invalid_schema",message:"Invalid configured project trust"};
    const project = object(value);
    const trust = project.trust_level ?? project.trustLevel;
    if (trust !== undefined && trust !== null && trust !== "trusted")
      throw {code:"provider_refused",message:"Configured project trust refuses the restricted CLI exception"};
  }
}

export function codexNeedsWindowsSandboxSelection(raw: unknown): boolean {
  const root = object(raw);
  if (root.config === null || typeof root.config !== "object" || Array.isArray(root.config))
    throw {code:"invalid_schema",message:"Windows sandbox configuration was not reported"};
  const config = object(root.config);
  const windows = object(config.windows);
  if (config.windows != null && (typeof config.windows !== "object" || Array.isArray(config.windows)))
    throw {code:"invalid_schema",message:"Invalid Windows sandbox configuration"};
  if (windows.sandbox != null) {
    if (!["mxc", "elevated", "unelevated"].includes(String(windows.sandbox)))
      throw {code:"invalid_schema",message:"Unknown configured Windows sandbox"};
    return false;
  }
  if (windows.allow_mxc !== undefined) {
    if (typeof windows.allow_mxc !== "boolean") throw {code:"invalid_schema",message:"Invalid Windows MXC policy"};
    if (!windows.allow_mxc) return false;
  }
  if (config.features != null && (typeof config.features !== "object" || Array.isArray(config.features)))
    throw {code:"invalid_schema",message:"Invalid Windows sandbox features"};
  const features = object(config.features);
  for (const key of windowsSandboxFeatures) {
    if (features[key] !== undefined) {
      if (typeof features[key] !== "boolean") throw {code:"invalid_schema",message:"Invalid Windows sandbox feature selection"};
      return false;
    }
  }
  return true;
}

export function codexWindowsSandboxOverride(raw: unknown): "mxc" | undefined {
  const root = object(raw);
  if (root.requirements === null) return "mxc";
  if (typeof root.requirements !== "object" || Array.isArray(root.requirements))
    throw {code:"invalid_schema",message:"Windows sandbox requirements were not reported"};
  const requirements = object(root.requirements);
  const allowed = requirements.allowedWindowsSandboxImplementations;
  if (allowed !== null && (!Array.isArray(allowed) || allowed.some(mode => !["mxc", "elevated", "unelevated"].includes(String(mode)))))
    throw {code:"invalid_schema",message:"Windows sandbox implementation requirements are unknown"};
  if (Array.isArray(allowed) && !allowed.includes("mxc")) return undefined;
  const modes = requirements.allowedSandboxModes;
  if (modes !== null && (!Array.isArray(modes) || modes.some(mode => !["read-only", "workspace-write", "danger-full-access"].includes(String(mode)))))
    throw {code:"invalid_schema",message:"Sandbox mode requirements are unknown"};
  if (Array.isArray(modes) && !modes.includes("workspace-write")) return undefined;
  if (requirements.defaultPermissions != null || requirements.allowedPermissionProfiles != null) return undefined;
  if (requirements.featureRequirements != null && (typeof requirements.featureRequirements !== "object" || Array.isArray(requirements.featureRequirements)))
    throw {code:"invalid_schema",message:"Windows sandbox feature requirements are unknown"};
  const features = object(requirements.featureRequirements);
  if (windowsSandboxFeatures.some(key => features[key] !== undefined)) return undefined;
  return "mxc";
}

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

export function codexWorkspaceWriteConfirmed(raw: unknown): boolean {
  const root = object(raw);
  const settings = root.threadSettings ? object(root.threadSettings) : root;
  const thread = object(settings.thread);
  const sandbox = object(settings.sandboxPolicy ?? settings.sandbox ?? thread.sandboxPolicy ?? thread.sandbox);
  return codexReportedSettings(raw).mode === "workspace-write" && sandbox.type === "workspaceWrite"
    && (settings.approvalPolicy ?? thread.approvalPolicy) === "never" && sandbox.networkAccess === false;
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

export function codexRestrictedCliInstructions(ctx: RuntimeContext, instructions?: string): string | undefined {
  if (!ctx.restrictedCli) return instructions;
  codexRestrictedCliConfig(ctx);
  const executable = ctx.restrictedCli.executable;
  return [instructions, `Trusted Domios CLI executable: ${executable}. Use this exact bare command token with normal Domios arguments in the native command tool. Do not add a PowerShell call operator, executable quotes, environment assignments or another interpreter. Single-quote opaque string arguments, especially IDs beginning with digits; keep the executable itself unquoted. Keep the inherited connection and credential; server authorization decides allowed operations. This exception applies only to the trusted CLI; other commands retain their sandbox.`].filter(value=>value!==undefined).join("\n\n");
}
