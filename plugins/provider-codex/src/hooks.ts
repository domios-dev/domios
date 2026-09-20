// Hook-file artefacts CodeTerm owns in the user's global ~/.codex. Split out of
// plugin.ts to keep that file under the repository's hard 1200-line limit.
import type { CodexHookEntry } from "./plugin";

export const CODETERM_CODEX_HOOK_MARKER = "codeterm-codex-session-binding-v1";
export const CODETERM_CODEX_PROFILE_CLEANUP_MARKER = "codeterm-profile-cleanup-v1";

/// Per-event budget Codex grants the hook. A Windows PowerShell cold start can
/// exceed the old 5s under load, which killed the session-start report before it
/// reached the daemon; the budget must cover the cold start, not the request.
export const CODEX_HOOK_TIMEOUT_SECS = 60;

/// The helper script's own HTTP budget. Separate from the hook budget: it bounds
/// one request once PowerShell is up, not the cold start around it.
export const CODEX_HOOK_REQUEST_TIMEOUT_SECS = 30;

export function codexWindowsHookScript(): string {
  return [
    "# " + CODETERM_CODEX_HOOK_MARKER,
    "# " + CODETERM_CODEX_PROFILE_CLEANUP_MARKER,
    'param([string]$HookEvent = "session_start")',
    '$ErrorActionPreference = "SilentlyContinue"',
    "$tab = $env:CODETERM_TAB_ID",
    "if ([string]::IsNullOrWhiteSpace($tab) -or [string]::IsNullOrWhiteSpace($env:CODETERM_API_URL) -or [string]::IsNullOrWhiteSpace($env:CODETERM_API_TOKEN)) { exit 0 }",
    '$base = $env:CODETERM_API_URL.TrimEnd("/")',
    "$headers = @{",
    '  "Content-Type" = "application/json"',
    '  "X-CodeTerm-Agent-Provider" = "codex"',
    "}",
    "if ($env:CODETERM_SESSION_BINDING_NONCE) {",
    '  $headers["X-CodeTerm-Binding-Nonce"] = $env:CODETERM_SESSION_BINDING_NONCE',
    "}",
    '$headers["Authorization"] = "Bearer $($env:CODETERM_API_TOKEN)"',
    "$body = [Console]::In.ReadToEnd()",
    'Invoke-RestMethod -Method Post -Uri "$base/api/tabs/$tab/hook/$HookEvent" -Headers $headers -Body $body -TimeoutSec ' + CODEX_HOOK_REQUEST_TIMEOUT_SECS + ' | Out-Null',
    'if ($HookEvent -eq "session_start") {',
    "  $profileSuffix = $tab -replace '[^A-Za-z0-9_-]', '-'",
    '  $profilePath = Join-Path $env:USERPROFILE (".codex\\codeterm-$profileSuffix.config.toml")',
    "  Remove-Item -LiteralPath $profilePath -Force -ErrorAction SilentlyContinue",
    "}",
    "exit 0",
    "",
  ].join("\n");
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
  if (hook._source !== undefined) return hook._source === CODETERM_CODEX_HOOK_MARKER;
  const cmd = String(hook.command || "");
  return (cmd.indexOf("/hook/session_start") !== -1 || cmd.indexOf("/hook/stop") !== -1)
    && cmd.indexOf("X-CodeTerm-Agent-Provider: codex") !== -1;
}

/// Tokens a CodeTerm-owned Codex hook script must carry to be current. This is
/// the hook's contract, not its prose: it is instance-agnostic by construction,
/// so dev and prod satisfy it identically.
export const CODEX_HOOK_SCRIPT_CONTRACT = [
  CODETERM_CODEX_HOOK_MARKER,
  CODETERM_CODEX_PROFILE_CLEANUP_MARKER,
  "$HookEvent",
  "CODETERM_TAB_ID",
  "CODETERM_API_URL",
  "CODETERM_API_TOKEN",
  "CODETERM_SESSION_BINDING_NONCE",
  "/api/tabs/",
  "-TimeoutSec " + CODEX_HOOK_REQUEST_TIMEOUT_SECS,
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
  return typeof text === "string" && isCurrentCodexHookScriptContent(text);
}

