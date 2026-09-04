// Hook-file artefacts CodeTerm owns in the user's global ~/.codex. Split out of
// plugin.ts to keep that file under the repository's hard 1200-line limit.
import type { CodexHookEntry } from "./plugin";

export const CODETERM_CODEX_HOOK_MARKER = "codeterm-codex-session-binding-v1";
export const CODETERM_CODEX_PROFILE_CLEANUP_MARKER = "codeterm-profile-cleanup-v1";

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
    'Invoke-RestMethod -Method Post -Uri "$base/api/tabs/$tab/hook/$HookEvent" -Headers $headers -Body $body -TimeoutSec 5 | Out-Null',
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
    const hook = hooks[i];
    if (!hook) continue;
    if (hook._source === CODETERM_CODEX_HOOK_MARKER) return true;
    const cmd = String(hook.command || "");
    if (
      (cmd.indexOf("/hook/session_start") !== -1 || cmd.indexOf("/hook/stop") !== -1) &&
      cmd.indexOf("X-CodeTerm-Agent-Provider: codex") !== -1
    ) {
      return true;
    }
  }
  return false;
}

export function isOwnedCodexWindowsHookScript(path: string): boolean {
  const text = host.fs.readFile(path);
  return typeof text === "string" && text === codexWindowsHookScript();
}

