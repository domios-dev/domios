import { commandIsOwned as isOwnedFrom1149 } from "./upgrade_from_1_14_9";

export interface HookCmd { type?: string; command?: string; timeout?: number; _source?: string }
export interface HookEntry { matcher?: string; hooks?: HookCmd[] }
export interface ClaudeSettings { hooks?: Record<string, HookEntry[]> }

export const DOMIOS_HOOK_MARKER = "domios-hook-v4";
export const DOMIOS_HOOK_TIMEOUT_SECONDS = 5;
export const DOMIOS_HOOK_MARKER_HEADER = "X-Domios-Hook: domios-hook-v4";
export const DOMIOS_HOOK_MARKER_ARG = '-H "' + DOMIOS_HOOK_MARKER_HEADER + '"';
export const INTEGRATION_HOOK_KEYS = ["PermissionRequest", "Notification", "Stop", "PreCompact", "SessionStart"];

export function commandCarriesMarker(command: string): boolean {
  return command.indexOf(DOMIOS_HOOK_MARKER_HEADER) !== -1;
}

export function entryCarriesMarker(entry: HookEntry): boolean {
  const hooks = entry && entry.hooks;
  return Array.isArray(hooks) && hooks.some((hook) => {
    return typeof hook.command === "string" && commandCarriesMarker(hook.command);
  });
}

export function entryIs1149Hook(entry: HookEntry): boolean {
  const hooks = entry && entry.hooks;
  return Array.isArray(hooks) && hooks.some((hook) => {
    return typeof hook.command === "string" && isOwnedFrom1149(hook.command);
  });
}

export function isCurrentHookCommand(command: string, platform: string): boolean {
  const executable = platform === "windows" ? "curl.exe " : "curl ";
  return command.indexOf(executable) === 0
    && command.indexOf(" --request POST ") !== -1
    && command.indexOf(" --data-binary ") !== -1
    && command.indexOf("if [") === -1
    && command.indexOf("|| true") === -1;
}

export function isDomiosHook(entry: HookEntry): boolean {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  return hooks.some((hook) => {
    if (!hook || typeof hook.command !== "string") return false;
    return commandCarriesMarker(hook.command) || isOwnedFrom1149(hook.command);
  });
}

export function entriesCarryMarker(entries: HookEntry[]): boolean {
  return entries.every((entry) => {
    if (!isDomiosHook(entry)) return true;
    return entryCarriesMarker(entry);
  });
}

export function entriesHaveBoundedTimeout(entries: HookEntry[]): boolean {
  return entries.every((entry) => (entry.hooks || []).every((hook) => {
    return !isDomiosHook({ hooks: [hook] }) || hook.timeout === DOMIOS_HOOK_TIMEOUT_SECONDS;
  }));
}
