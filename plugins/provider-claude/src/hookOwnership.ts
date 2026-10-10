export interface HookCmd { type?: string; command?: string; timeout?: number; _source?: string }
export interface HookEntry { matcher?: string; hooks?: HookCmd[] }

export const CODETERM_HOOK_MARKER = "codeterm-hook-v3";
export const CODETERM_HOOK_MARKER_HEADER = "X-CodeTerm-Hook: codeterm-hook-v3";
export const CODETERM_HOOK_MARKER_ARG = '-H "' + CODETERM_HOOK_MARKER_HEADER + '"';
export const INTEGRATION_HOOK_KEYS = ["PermissionRequest", "Notification", "Stop", "PreCompact", "SessionStart"];

const CODETERM_CURRENT_ROUTES = [
  "/api/tabs/$CODETERM_TAB_ID/hook/permission",
  "/api/tabs/$CODETERM_TAB_ID/hook/stop",
  "/api/tabs/$CODETERM_TAB_ID/hook/session_start",
  "$CODETERM_API_URL/api/mem/flush",
];

const LEGACY_V2_SESSION_START =
  'curl -s -X POST http://localhost:${CODETERM_API_PORT:-7685}/api/panes/$CODETERM_PANE_ID/hook/session_start '
  + "-H 'Content-Type: application/json' -H \"X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}\" "
  + "-d '{}' > /dev/null 2>&1 || true";
const LEGACY_V2_STOP =
  'curl -s -X POST http://localhost:${CODETERM_API_PORT:-7685}/api/panes/$CODETERM_PANE_ID/hook/stop '
  + "-H 'Content-Type: application/json' -d '{}' > /dev/null 2>&1 || true";
const LEGACY_V2_COMMANDS = new Set([LEGACY_V2_SESSION_START, LEGACY_V2_STOP]);

export interface ClaudeSettings { hooks?: Record<string, HookEntry[]> }

export function commandCarriesMarker(command: string): boolean {
  return command.indexOf(CODETERM_HOOK_MARKER_HEADER) !== -1;
}

export function entryCarriesMarker(entry: HookEntry): boolean {
  const hooks = entry && entry.hooks;
  return Array.isArray(hooks) && hooks.some((hook) => {
    return typeof hook.command === "string" && commandCarriesMarker(hook.command);
  });
}

export function entryIsLegacyPaneCodetermHook(entry: HookEntry): boolean {
  const hooks = entry && entry.hooks;
  return Array.isArray(hooks) && hooks.some((hook) => {
    return typeof hook.command === "string" && LEGACY_V2_COMMANDS.has(hook.command);
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

function commandIsCurrentUnmarked(command: string): boolean {
  if (command.indexOf("$CODETERM_API_URL") === -1) return false;
  if (command.indexOf("$CODETERM_TAB_ID") === -1) return false;
  if (command.indexOf("$CODETERM_API_TOKEN") === -1) return false;
  return CODETERM_CURRENT_ROUTES.some((route) => command.indexOf(route) !== -1);
}

export function isCodetermHook(entry: HookEntry): boolean {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  return hooks.some((hook) => {
    if (!hook) return false;
    if (typeof hook._source === "string"
        && (hook._source.indexOf("codeterm-hook") === 0 || hook._source === "codeterm")) {
      return true;
    }
    const command = hook.command;
    return typeof command === "string"
      && (commandCarriesMarker(command)
        || commandIsCurrentUnmarked(command)
        || LEGACY_V2_COMMANDS.has(command));
  });
}

export function entriesCarryMarker(entries: HookEntry[]): boolean {
  return entries.every((entry) => {
    if (!isCodetermHook(entry)) return true;
    const hooks = (entry && entry.hooks) || [];
    return hooks.some((hook) => {
      return typeof hook.command === "string" && commandCarriesMarker(hook.command);
    });
  });
}
