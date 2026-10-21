// Hook-file artifacts Domios owns in the user's global ~/.codex.
import type { CodexHookEntry } from "./plugin";
import {
  commandIsOwned as isOwnedFrom1149,
  scriptIsOwned as scriptIsOwnedFrom1149,
} from "./upgrade_from_1_14_9";

export const DOMIOS_CODEX_HOOK_MARKER = "domios-hook-v4";
export const DOMIOS_CODEX_HOOK_MARKER_HEADER = "X-Domios-Hook: domios-hook-v4";

/// Per-event budget includes hook process startup and the daemon request.
export const CODEX_HOOK_TIMEOUT_SECS = 60;

/// The Windows helper's curl limit is separate from Codex's hook deadline.
export const CODEX_HOOK_REQUEST_TIMEOUT_SECS = 30;

export function codexWindowsHookScript(): string {
  return [
    "@echo off",
    "setlocal DisableDelayedExpansion",
    "rem " + DOMIOS_CODEX_HOOK_MARKER,
    "if not defined DOMIOS_TAB_ID exit /b 0",
    "if not defined DOMIOS_API_URL exit /b 0",
    "if not defined DOMIOS_API_TOKEN exit /b 0",
    "if not defined DOMIOS_SESSION_BINDING_NONCE exit /b 0",
    'set "hookEvent=%~1"',
    'set "hookMarker=%~2 %~3"',
    'if /I not "%hookMarker%"=="X-Domios-Hook: ' + DOMIOS_CODEX_HOOK_MARKER + '" exit /b 0',
    'if /I "%hookEvent%"=="session_start" goto post_event',
    'if /I "%hookEvent%"=="stop" goto post_event',
    "exit /b 0",
    ":post_event",
    'set "apiBase=%DOMIOS_API_URL%"',
    'if "%apiBase:~-1%"=="/" set "apiBase=%apiBase:~0,-1%"',
    'set "responseFile=%TEMP%\\domios-hook-%DOMIOS_TAB_ID%-%hookEvent%-%RANDOM%.tmp"',
    'curl.exe --silent --show-error --max-time ' + CODEX_HOOK_REQUEST_TIMEOUT_SECS + ' --request POST "%apiBase%/api/tabs/%DOMIOS_TAB_ID%/hook/%hookEvent%" --header "Content-Type: application/json" --header "X-Domios-Agent-Provider: codex" --header "%hookMarker%" --header "X-Domios-Binding-Nonce: %DOMIOS_SESSION_BINDING_NONCE%" --header "Authorization: Bearer %DOMIOS_API_TOKEN%" --output "%responseFile%" --data-binary @-',
    'if exist "%responseFile%" del /F /Q "%responseFile%"',
    "exit /b 0",
    "",
  ].join("\r\n");
}

function isOwnedCodexHook(hook: NonNullable<CodexHookEntry["hooks"]>[number] | undefined): boolean {
  if (!hook) return false;
  const command = String(hook.command || "");
  const windowsCommand = String(hook.commandWindows || "");
  return command.includes(DOMIOS_CODEX_HOOK_MARKER_HEADER)
    || windowsCommand.includes(DOMIOS_CODEX_HOOK_MARKER_HEADER)
    || isOwnedFrom1149(command)
    || isOwnedFrom1149(windowsCommand);
}

export function is1149CodexHook(entry: CodexHookEntry): boolean {
  const hooks = entry && entry.hooks;
  return Array.isArray(hooks) && hooks.some((hook) => {
    return isOwnedFrom1149(String(hook?.command || ""))
      || isOwnedFrom1149(String(hook?.commandWindows || ""));
  });
}

export function is1149CodexHookCommand(command: string): boolean {
  return isOwnedFrom1149(command);
}

export function isDomiosCodexHook(entry: CodexHookEntry): boolean {
  const hooks = entry && entry.hooks;
  return Array.isArray(hooks) && hooks.some(isOwnedCodexHook);
}

export function removeOwnedCodexHooks(entries: CodexHookEntry[]): CodexHookEntry[] {
  const out: CodexHookEntry[] = [];
  for (const entry of entries) {
    const hooks = entry && entry.hooks;
    if (!Array.isArray(hooks)) {
      out.push(entry);
      continue;
    }
    const kept = hooks.filter((hook) => !isOwnedCodexHook(hook));
    if (kept.length === hooks.length) out.push(entry);
    else if (kept.length > 0) out.push({ ...entry, hooks: kept });
  }
  return out;
}

export const DOMIOS_CODEX_HOOK_SCRIPT_CONTRACT = [
  "rem " + DOMIOS_CODEX_HOOK_MARKER,
  "hookEvent",
  "DOMIOS_TAB_ID",
  "DOMIOS_API_URL",
  "DOMIOS_API_TOKEN",
  "DOMIOS_SESSION_BINDING_NONCE",
  DOMIOS_CODEX_HOOK_MARKER_HEADER,
  "/api/tabs/",
  "curl.exe",
  "--max-time " + CODEX_HOOK_REQUEST_TIMEOUT_SECS,
  "responseFile",
  "--output",
];

export function hasAdequateCodexHookTimeout(entry: CodexHookEntry | undefined): boolean {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks) || hooks.length === 0) return false;
  return hooks.every((hook) => {
    const timeout = hook && hook.timeout;
    return typeof timeout === "number" && Number.isFinite(timeout)
      && timeout >= CODEX_HOOK_TIMEOUT_SECS;
  });
}

export function codexHookContentHash(text: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = (hash + ((hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24))) >>> 0;
  }
  return ("0000000" + hash.toString(16)).slice(-8);
}

export function isOwnedCodexHookScriptContent(text: string): boolean {
  return text.indexOf(DOMIOS_CODEX_HOOK_MARKER) !== -1
    || scriptIsOwnedFrom1149(text);
}

export function is1149CodexHookScriptContent(text: string): boolean {
  return scriptIsOwnedFrom1149(text);
}

export function isCurrentCodexHookScriptContent(text: string): boolean {
  const normalizeLineEndings = (value: string) => value.split("\r\n").join("\n").split("\r").join("\n");
  return normalizeLineEndings(text) === normalizeLineEndings(codexWindowsHookScript());
}

export function isOwnedCodexWindowsHookScript(path: string): boolean {
  const text = host.fs.readFile(path);
  return typeof text === "string" && isOwnedCodexHookScriptContent(text);
}
