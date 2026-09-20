// Hook-file artefacts CodeTerm owns in the user's global ~/.codex. Split out of
// plugin.ts to keep that file under the repository's hard 1200-line limit.
import type { CodexHookEntry } from "./plugin";

export const CODETERM_CODEX_HOOK_MARKER = "codeterm-codex-session-binding-v1";
export const CODETERM_CODEX_PROFILE_CLEANUP_MARKER = "codeterm-profile-cleanup-v1";
const LEGACY_CODEX_HOOK_COMMANDS = new Set([
  'curl -s -X POST http://localhost:${CODETERM_API_PORT:-7686}/api/tabs/$CODETERM_TAB_ID/hook/session_start -H \'Content-Type: application/json\' -H \'X-CodeTerm-Agent-Provider: codex\' -H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}" -H "Authorization: Bearer ${CODETERM_API_TOKEN}" --data-binary @- > /dev/null 2>&1 || true',
  'curl -s -X POST http://localhost:${CODETERM_API_PORT:-7685}/api/panes/$CODETERM_PANE_ID/hook/session_start -H \'Content-Type: application/json\' -H \'X-CodeTerm-Agent-Provider: codex\' --data-binary @- > /dev/null 2>&1 || true',
]);
for (const event of ["session_start", "stop"]) {
  LEGACY_CODEX_HOOK_COMMANDS.add(
    'if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then '
      + 'curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/' + event + '" '
      + "-H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' "
      + '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}" '
      + '-H "Authorization: Bearer ${CODETERM_API_TOKEN}" '
      + "--data-binary @- > /dev/null 2>&1 || true; fi",
  );
}

/// Per-event budget includes hook process startup and the daemon request.
export const CODEX_HOOK_TIMEOUT_SECS = 60;

/// The Windows helper's curl limit is separate from Codex's hook deadline.
export const CODEX_HOOK_REQUEST_TIMEOUT_SECS = 30;

export function codexWindowsHookScript(): string {
  return [
    "@echo off",
    "setlocal DisableDelayedExpansion",
    "rem " + CODETERM_CODEX_HOOK_MARKER,
    "rem " + CODETERM_CODEX_PROFILE_CLEANUP_MARKER,
    "if not defined CODETERM_TAB_ID exit /b 0",
    "if not defined CODETERM_API_URL exit /b 0",
    "if not defined CODETERM_API_TOKEN exit /b 0",
    "if not defined CODETERM_SESSION_BINDING_NONCE exit /b 0",
    'set "hookEvent=%~1"',
    'set "hookMarker=%~2 %~3"',
    'if /I not "%hookMarker%"=="X-CodeTerm-Hook: ' + CODETERM_CODEX_HOOK_MARKER + '" exit /b 0',
    'if /I "%hookEvent%"=="session_start" goto post_event',
    'if /I "%hookEvent%"=="stop" goto post_event',
    "exit /b 0",
    ":post_event",
    'set "apiBase=%CODETERM_API_URL%"',
    'if "%apiBase:~-1%"=="/" set "apiBase=%apiBase:~0,-1%"',
    'set "responseFile=%TEMP%\\codeterm-hook-%CODETERM_TAB_ID%-%hookEvent%-%RANDOM%.tmp"',
    'curl.exe --silent --show-error --max-time ' + CODEX_HOOK_REQUEST_TIMEOUT_SECS + ' --request POST "%apiBase%/api/tabs/%CODETERM_TAB_ID%/hook/%hookEvent%" --header "Content-Type: application/json" --header "X-CodeTerm-Agent-Provider: codex" --header "%hookMarker%" --header "X-CodeTerm-Binding-Nonce: %CODETERM_SESSION_BINDING_NONCE%" --header "Authorization: Bearer %CODETERM_API_TOKEN%" --output "%responseFile%" --data-binary @-',
    'if exist "%responseFile%" del /F /Q "%responseFile%"',
    'if /I "%hookEvent%"=="session_start" if defined USERPROFILE if exist "%USERPROFILE%\\.codex\\codeterm-%CODETERM_TAB_ID%.config.toml" del /F /Q "%USERPROFILE%\\.codex\\codeterm-%CODETERM_TAB_ID%.config.toml"',
    "exit /b 0",
    "",
  ].join("\r\n");
}

export function isCodetermCodexHook(entry: CodexHookEntry): boolean {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (let i = 0; i < hooks.length; i++) {
    if (isOwnedCodexHook(hooks[i])) return true;
  }
  return false;
}

export function removeOwnedCodexHooks(entries: CodexHookEntry[]): CodexHookEntry[] {
  const out: CodexHookEntry[] = [];
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
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

function isOwnedCodexHook(hook: NonNullable<CodexHookEntry["hooks"]>[number] | undefined): boolean {
  if (!hook) return false;
  const command = String(hook.command || "");
  const fullCommand = command + "\n" + String(hook.commandWindows || "");
  if (fullCommand.indexOf("X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER) !== -1
    || fullCommand.indexOf("X-CodeTerm-Hook:" + CODETERM_CODEX_HOOK_MARKER) !== -1) return true;
  if (hook._source !== undefined) return hook._source === CODETERM_CODEX_HOOK_MARKER;
  return LEGACY_CODEX_HOOK_COMMANDS.has(command);
}

/// Tokens a CodeTerm-owned Codex hook script must carry to be current. This is
/// the hook's contract, not its prose: it is instance-agnostic by construction,
/// so dev and prod satisfy it identically.
export const CODEX_HOOK_SCRIPT_CONTRACT = [
  "rem " + CODETERM_CODEX_HOOK_MARKER,
  CODETERM_CODEX_PROFILE_CLEANUP_MARKER,
  "hookEvent",
  "CODETERM_TAB_ID",
  "CODETERM_API_URL",
  "CODETERM_API_TOKEN",
  "CODETERM_SESSION_BINDING_NONCE",
  "X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER,
  "/api/tabs/",
  "curl.exe",
  "--max-time " + CODEX_HOOK_REQUEST_TIMEOUT_SECS,
  "responseFile",
  "--output",
];

/// True when every command in a CodeTerm-owned entry grants the hook the
/// cold-start-tolerant budget. A hook still carrying the old 5s budget reads as
/// stale so Repair rewrites it — otherwise the fix would never reach an install
/// that predates it.
export function hasAdequateCodexHookTimeout(entry: CodexHookEntry | undefined): boolean {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks) || hooks.length === 0) return false;
  return hooks.every((hook) => {
    const timeout = hook && hook.timeout;
    return typeof timeout === "number" && Number.isFinite(timeout)
      && timeout >= CODEX_HOOK_TIMEOUT_SECS;
  });
}

/**
 * Stable hash of hook content. Codex trusts a hook by content hash, so content
 * is what currency is decided on — never which instance wrote it. Byte-identical
 * content therefore hashes the same for every instance on the machine.
 */
export function codexHookContentHash(text: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = (hash + ((hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24))) >>> 0;
  }
  return ("0000000" + hash.toString(16)).slice(-8);
}

export function isOwnedCodexHookScriptContent(text: string): boolean {
  return text.indexOf(CODETERM_CODEX_HOOK_MARKER) !== -1;
}

/// Current when the content satisfies the contract and bakes in no instance.
/// A build that merely reformats the script does not make the other build's
/// file stale, which is what used to cost the user a re-trust on every switch.
export function isCurrentCodexHookScriptContent(text: string): boolean {
  if (!isOwnedCodexHookScriptContent(text)) return false;
  if (/:\d{2,5}/.test(text)) return false;
  // An instance fallback (a port variable or a loopback host) routes prod agents to dev.
  if (text.indexOf("CODETERM_API_PORT") !== -1 || /127\.0\.0\.1|localhost/i.test(text)) return false;
  for (let i = 0; i < CODEX_HOOK_SCRIPT_CONTRACT.length; i++) {
    if (text.indexOf(CODEX_HOOK_SCRIPT_CONTRACT[i]) === -1) return false;
  }
  return true;
}

export function isOwnedCodexWindowsHookScript(path: string): boolean {
  const text = host.fs.readFile(path);
  return typeof text === "string" && isOwnedCodexHookScriptContent(text);
}

