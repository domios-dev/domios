import type { CodexHookEntry } from "./plugin";
import { grantCodexHookTrust, inspectCodexHookTrust } from "./appServer";
import {
  codexHookContentHash,
  codexWindowsHookScript,
  isCodetermCodexHook,
  removeOwnedCodexHooks,
  isOwnedCodexWindowsHookScript,
  isCurrentCodexHookScriptContent,
  hasAdequateCodexHookTimeout,
  CODETERM_CODEX_HOOK_MARKER,
  CODEX_HOOK_TIMEOUT_SECS,
} from "./hooks";

declare const host: {
  homeDir(): string | null;
  platform(): string;
  log(level: "trace" | "debug" | "info" | "warn" | "error", message: string): void;
  fs: {
    fileExists(path: string): boolean;
    readFile(path: string): string | null;
    readJson<T>(path: string): T | null;
    writeFile(path: string, contents: string): boolean;
    writeJson(path: string, value: unknown): boolean;
    removeFile(path: string): boolean;
  };
};

export interface CodexIntegrationInstallOptions { consent?: boolean }

export const CODEX_INTEGRATION_STATE_ABSENT = "absent";
export const CODEX_INTEGRATION_STATE_STALE_VERSION = "stale_version";
export const CODEX_INTEGRATION_STATE_UNTRUSTED_HASH = "untrusted_hash";
export const CODEX_INTEGRATION_STATE_INSTALLED = "installed";

type CodexHookTrustVerdict = "trusted" | "stale";
interface CodexHookTrustCacheEntry {
  fingerprint: string;
  hookFingerprint: string;
  trustFingerprint: string;
  verdict: CodexHookTrustVerdict;
}
const codexHookTrustCache = new Map<string, CodexHookTrustCacheEntry>();

interface CodexHooks {
  hooks?: { SessionStart?: CodexHookEntry[]; Stop?: CodexHookEntry[] };
}

interface IntegrationFileSnapshot {
  path: string;
  existed: boolean;
  bytes: string;
}

export function checkIntegration(): unknown {
  const path = codexHooksPath();
  if (!path || !host.fs.fileExists(path)) {
    return emptyIntegrationStatus(path, false);
  }
  const json = host.fs.readJson<CodexHooks>(path);
  if (!json) {
    return emptyIntegrationStatus(path, true);
  }
  const arr = json.hooks && json.hooks.SessionStart;
  const stopArr = json.hooks && json.hooks.Stop;
  const installed = Array.isArray(arr) && arr.some(isCodetermCodexHook);
  const stopInstalled = Array.isArray(stopArr) && stopArr.some(isCodetermCodexHook)
    && (host.platform() !== "windows" || stopArr.some(hasCodetermCodexWindowsCommand));
  const winScript = codexWindowsHookScriptPath();
  const winScriptText = winScript && host.fs.fileExists(winScript)
    ? String(host.fs.readFile(winScript) || "")
    : "";
  const windowsReady = host.platform() !== "windows" || (
    !!winScript &&
    Array.isArray(arr) &&
    arr.some(hasCodetermCodexWindowsCommand)
  );
  const contractOk = host.platform() === "windows"
    ? isCurrentCodexHookScriptContent(winScriptText)
    : (Array.isArray(arr) && arr.some((entry) => {
        const hk = entry && entry.hooks;
        return Array.isArray(hk) && hk.some((h) => {
          const command = String(h?.command || "");
          return command.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1 &&
            command.indexOf("CODETERM_TAB_ID") !== -1 &&
            command.indexOf("CODETERM_API_URL") !== -1 &&
            command.indexOf("CODETERM_API_TOKEN") !== -1 &&
            command.indexOf("X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER) !== -1 &&
            !hasLiteralPort(command) &&
            command.indexOf("/api/tabs/") !== -1;
        });
      }));
  const current = installed && windowsReady && contractOk && hookTimeoutsAdequate(arr, stopArr);
  const trustState = codexHookTrustState(path);
  const trustOk = trustState === "enabled" || trustState === "trusted";
  const integrationState = !installed
    ? CODEX_INTEGRATION_STATE_ABSENT
    : (trustState === "unknown"
      ? "unknown"
      : (!trustOk || trustState === "stale" || trustState === "missing"
        ? CODEX_INTEGRATION_STATE_UNTRUSTED_HASH
        : (!current || !stopInstalled ? CODEX_INTEGRATION_STATE_STALE_VERSION : CODEX_INTEGRATION_STATE_INSTALLED)));
  return {
    hooks_file_exists: true,
    hook_content_hash: ownedContentHash(arr, stopArr, winScriptText),
    hooks_installed: current && stopInstalled && trustOk,
    session_start_hook: current && trustOk,
    stop_hook: stopInstalled && trustOk,
    hook_trust_state: trustState,
    hook_trust_enabled: trustState === "enabled" ? true : (trustState === "disabled" ? false : null),
    integration_state: integrationState,
    repair_requires_consent: integrationState !== CODEX_INTEGRATION_STATE_INSTALLED && integrationState !== "unknown",
    hooks_owned: installed,
    hook_contract_current: current && stopInstalled,
    target: path,
    backup_exists: host.fs.fileExists(path + ".codeterm-backup"),
    proposed_changes: ["SessionStart hook", "Stop hook", "hook trust"],
  };
}

/// The content Codex actually trusts: the CodeTerm-owned hook commands plus the
/// helper script. Two instances writing byte-identical content hash the same,
/// which is what makes the verdict independent of who wrote it.
function ownedContentHash(
  sessionStart: CodexHookEntry[] | undefined,
  stop: CodexHookEntry[] | undefined,
  scriptText: string,
): string {
  const parts: string[] = [];
  for (const entries of [sessionStart, stop]) {
    if (!Array.isArray(entries)) continue;
    for (const entry of entries) {
      if (!isCodetermCodexHook(entry)) continue;
      const hooks = (entry && entry.hooks) || [];
      for (const hook of hooks) {
        parts.push(String(hook?.command || ""));
        parts.push(String(hook?.commandWindows || ""));
      }
    }
  }
  parts.push(scriptText);
  return codexHookContentHash(parts.join(String.fromCharCode(0)));
}

function emptyIntegrationStatus(path: string | null, fileExists: boolean): unknown {
  return {
    hooks_file_exists: fileExists,
    hook_content_hash: codexHookContentHash(""),
    hooks_installed: false,
    session_start_hook: false,
    stop_hook: false,
    hook_trust_state: "missing",
    hook_trust_enabled: null,
    integration_state: CODEX_INTEGRATION_STATE_ABSENT,
    repair_requires_consent: true,
    hooks_owned: false,
    hook_contract_current: false,
    target: path || "~/.codex/hooks.json",
    backup_exists: !!path && host.fs.fileExists(path + ".codeterm-backup"),
    proposed_changes: ["SessionStart hook", "Stop hook", "hook trust"],
  };
}

export function installIntegration(
  apiPort: number,
  options?: CodexIntegrationInstallOptions,
): unknown {
  void apiPort;
  if (!options || options.consent !== true) {
    return { ok: false, error: "provider integration installation requires explicit local-user consent" };
  }
  const path = codexHooksPath();
  if (!path) return { ok: false, error: "could not resolve ~/.codex/hooks.json" };

  const hooksExisted = host.fs.fileExists(path);
  const originalHooksValue = hooksExisted ? host.fs.readFile(path) : null;
  if (hooksExisted && typeof originalHooksValue !== "string") {
    return { ok: false, error: "could not read existing hooks JSON at " + path };
  }
  const originalHooks = typeof originalHooksValue === "string" ? originalHooksValue : "";
  let parsedHooks: unknown = {};
  if (hooksExisted) {
    try {
      parsedHooks = JSON.parse(originalHooks);
    } catch (_) {
      return { ok: false, error: "existing hooks JSON is malformed at " + path };
    }
  }
  if (!parsedHooks || typeof parsedHooks !== "object" || Array.isArray(parsedHooks)) {
    return { ok: false, error: "existing hooks JSON must contain an object at " + path };
  }
  const json = parsedHooks as CodexHooks;
  if (json.hooks !== undefined && (typeof json.hooks !== "object" || Array.isArray(json.hooks))) {
    return { ok: false, error: "hooks is not an object" };
  }
  if (json.hooks?.SessionStart !== undefined && !Array.isArray(json.hooks.SessionStart)) {
    return { ok: false, error: "hooks.SessionStart is not an array" };
  }
  if (json.hooks?.Stop !== undefined && !Array.isArray(json.hooks.Stop)) {
    return { ok: false, error: "hooks.Stop is not an array" };
  }
  const configPath = codexConfigPath();
  const configExisted = !!configPath && host.fs.fileExists(configPath);
  const originalConfigValue = configPath && configExisted ? host.fs.readFile(configPath) : null;
  if (configExisted && typeof originalConfigValue !== "string") {
    return { ok: false, error: "could not read existing Codex config at " + configPath };
  }
  const originalConfig = typeof originalConfigValue === "string" ? originalConfigValue : "";
  const scriptPath = host.platform() === "windows" ? codexWindowsHookScriptPath() : null;
  const scriptExisted = !!scriptPath && host.fs.fileExists(scriptPath);
  const originalScriptValue = scriptExisted && scriptPath ? host.fs.readFile(scriptPath) : null;
  if (scriptExisted && typeof originalScriptValue !== "string") {
    return { ok: false, error: "could not read existing hook script at " + scriptPath };
  }
  const originalScript = typeof originalScriptValue === "string" ? originalScriptValue : "";
  if (scriptPath && scriptExisted && !isOwnedCodexWindowsHookScript(scriptPath)) {
    return { ok: false, error: "refusing to replace unowned hook script at " + scriptPath };
  }
  const ownershipPath = codexHookTrustOwnershipPath();
  const ownershipSnapshot = ownershipPath ? captureIntegrationFile(ownershipPath) : null;
  if (ownershipPath && !ownershipSnapshot) {
    return { ok: false, error: "could not read existing Codex trust ownership record" };
  }
  const artifactPaths = [
    path + ".codeterm-backup",
    path + ".codeterm-backup-state",
    ...(configPath && configExisted ? [configPath + ".codeterm-backup"] : []),
    ...(scriptPath ? [scriptPath + ".codeterm-backup", scriptPath + ".codeterm-backup-state"] : []),
  ];
  const artifactSnapshots = artifactPaths.map(captureIntegrationFile);
  if (artifactSnapshots.some((snapshot) => snapshot === null)) {
    return { ok: false, error: "could not snapshot existing CodeTerm integration artifacts" };
  }
  const rollbackSnapshots: IntegrationFileSnapshot[] = [
    { path, existed: hooksExisted, bytes: originalHooks },
    ...(configPath ? [{ path: configPath, existed: configExisted, bytes: originalConfig }] : []),
    ...(scriptPath ? [{ path: scriptPath, existed: scriptExisted, bytes: originalScript }] : []),
    ...(ownershipSnapshot ? [ownershipSnapshot] : []),
    ...(artifactSnapshots as IntegrationFileSnapshot[]),
  ];
  const fail = (error: string) => {
    const restored = restoreIntegrationFiles(rollbackSnapshots);
    return { ok: false, error: restored ? error : error + "; exact rollback failed" };
  };
  if (!host.fs.writeFile(path + ".codeterm-backup", originalHooks)
    || !host.fs.writeFile(path + ".codeterm-backup-state", hooksExisted ? "present" : "absent")) {
    return fail("could not write recoverable backup for " + path);
  }
  if (configPath && configExisted
    && !host.fs.writeFile(configPath + ".codeterm-backup", originalConfig)) {
    return fail("could not write recoverable backup for " + configPath);
  }
  if (scriptPath && (!host.fs.writeFile(scriptPath + ".codeterm-backup", originalScript)
    || !host.fs.writeFile(scriptPath + ".codeterm-backup-state", scriptExisted ? "present" : "absent"))) {
    return fail("could not write recoverable backup for " + scriptPath);
  }
  if (!json.hooks) json.hooks = {};
  for (const event of ["SessionStart", "Stop"] as const) {
    const existing = json.hooks[event];
    if (existing !== undefined && !Array.isArray(existing)) {
      return { ok: false, error: "hooks." + event + " is not an array" };
    }
    json.hooks[event] = removeOwnedCodexHooks(existing || []);
  }

  const authH = '-H "Authorization: Bearer ${CODETERM_API_TOKEN}"';
  const nonceH = '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"';
  const markerH = '-H "X-CodeTerm-Hook: ' + CODETERM_CODEX_HOOK_MARKER + '"';
  const guard = 'if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ] && [ -n "${CODETERM_SESSION_BINDING_NONCE:-}" ]; then ';
  const hookCmd = (route: string) => guard + 'curl -s -X POST "$CODETERM_API_URL'
    + "/api/tabs/$CODETERM_TAB_ID/hook/" + route + '" '
    + "-H 'Content-Type: application/json' "
    + "-H 'X-CodeTerm-Agent-Provider: codex' "
    + markerH + " "
    + nonceH + " "
    + authH
    + ' --data-binary @- > /dev/null 2>&1 || true; fi';
  const winCmd = (route: string) => scriptPath
    ? [
        "cmd.exe /d /s /c call",
        '"' + scriptPath + '"',
        route,
        "X-CodeTerm-Hook:",
        CODETERM_CODEX_HOOK_MARKER,
      ].join(" ")
    : hookCmd(route);

  if (scriptPath && !host.fs.writeFile(scriptPath, codexWindowsHookScript())) {
    return fail("writeFile failed for " + scriptPath);
  }

  json.hooks.SessionStart!.push({
    matcher: "startup|resume|clear|compact",
    hooks: [{
      type: "command",
      command: hookCmd("session_start"),
      commandWindows: winCmd("session_start"),
      timeout: CODEX_HOOK_TIMEOUT_SECS,
      _source: CODETERM_CODEX_HOOK_MARKER,
    }],
  });
  json.hooks.Stop!.push({
    hooks: [{
      type: "command",
      command: hookCmd("stop"),
      commandWindows: winCmd("stop"),
      timeout: CODEX_HOOK_TIMEOUT_SECS,
      _source: CODETERM_CODEX_HOOK_MARKER,
    }],
  });

  const ok = host.fs.writeJson(path, json);
  if (ok) {
    const trust = enableExistingCodexHookTrust(path);
    if (trust.write_failed) return fail("could not enable existing Codex hook trust");
    if (trust.changed) {
      if (!ownershipPath || !host.fs.writeFile(ownershipPath, path)) {
        return fail("could not record owned Codex hook trust expansion");
      }
    }
    host.log("info", "[codex-plugin] SessionStart + Stop hooks installed at " + path);
    return { ok: true, hook_trust_state: trust.state };
  }
  return fail("writeJson failed for " + path);
}

export function grantIntegrationTrust(
  managedKeys: string[],
  then: (result: unknown) => unknown = (result) => result,
): unknown {
  const hooksPath = codexHooksPath();
  if (!hooksPath) {
    return then({ granted: false, wrote: false, degraded: true, verify_failed: false, reason: "could not resolve ~/.codex/hooks.json" });
  }
  return grantCodexHookTrust({
    hooksPath,
    configPath: codexConfigPath(),
    managedLabels: Array.isArray(managedKeys) ? managedKeys.map(String) : [],
    listCwd: host.homeDir() || ".",
  }, (result) => {
    if (result.granted) cacheCodexHookTrust(hooksPath, "trusted");
    return then(result);
  });
}

export function uninstallIntegration(): unknown {
  const path = codexHooksPath();
  if (!path) return { ok: true };
  let hooksOk = true;
  if (host.fs.fileExists(path)) {
    const json = host.fs.readJson<CodexHooks>(path);
    if (!json || !json.hooks || typeof json.hooks !== "object") {
      return { ok: false, error: "could not safely read hooks JSON at " + path };
    }
    if (Array.isArray(json.hooks.SessionStart)) {
      json.hooks.SessionStart = removeOwnedCodexHooks(json.hooks.SessionStart);
    }
    if (Array.isArray(json.hooks.Stop)) {
      json.hooks.Stop = removeOwnedCodexHooks(json.hooks.Stop);
    }
    hooksOk = host.fs.writeJson(path, json);
  }
  const ownershipPath = codexHookTrustOwnershipPath();
  if (ownershipPath && host.fs.fileExists(ownershipPath)) {
    const ownedTarget = host.fs.readFile(ownershipPath);
    if (ownedTarget === path && disableOwnedCodexHookTrust(path)) {
      host.fs.removeFile(ownershipPath);
    }
  }
  const scriptPath = codexWindowsHookScriptPath();
  if (scriptPath && host.fs.fileExists(scriptPath) && isOwnedCodexWindowsHookScript(scriptPath)) {
    host.fs.removeFile(scriptPath);
  }
  return hooksOk ? { ok: true } : { ok: false, error: "writeJson failed for " + path };
}

export function restoreIntegration(): unknown {
  const path = codexHooksPath();
  if (!path) return { ok: false, error: "could not resolve ~/.codex/hooks.json" };
  const backup = path + ".codeterm-backup";
  if (!host.fs.fileExists(backup)) return { ok: false, error: "no recoverable backup at " + backup };
  const hooks = host.fs.readFile(backup);
  if (typeof hooks !== "string") return { ok: false, error: "restore failed for " + path };
  const hooksState = host.fs.readFile(path + ".codeterm-backup-state");
  if (hooksState === "absent" || (typeof hooksState !== "string" && hooks === "")) {
    if (host.fs.fileExists(path) && !host.fs.removeFile(path)) return { ok: false, error: "restore failed for " + path };
  } else if (!host.fs.writeFile(path, hooks)) {
    return { ok: false, error: "restore failed for " + path };
  }
  const config = codexConfigPath();
  if (config && host.fs.fileExists(config + ".codeterm-backup")) {
    const original = host.fs.readFile(config + ".codeterm-backup");
    if (typeof original !== "string" || !host.fs.writeFile(config, original)) return { ok: false, error: "restore failed for " + config };
    const ownershipPath = codexHookTrustOwnershipPath();
    if (ownershipPath && host.fs.fileExists(ownershipPath)) host.fs.removeFile(ownershipPath);
  }
  const script = host.platform() === "windows" ? codexWindowsHookScriptPath() : null;
  if (script && host.fs.fileExists(script + ".codeterm-backup")) {
    const original = host.fs.readFile(script + ".codeterm-backup");
    if (typeof original !== "string") return { ok: false, error: "restore failed for " + script };
    const scriptState = host.fs.readFile(script + ".codeterm-backup-state");
    if (scriptState === "absent" || (typeof scriptState !== "string" && original === "")) {
      if (host.fs.fileExists(script) && !host.fs.removeFile(script)) return { ok: false, error: "restore failed for " + script };
    } else if (!host.fs.writeFile(script, original)) {
      return { ok: false, error: "restore failed for " + script };
    }
  }
  return { ok: true };
}

export function ensureIntegrationOnStartup(
  apiPort: number,
  install: (apiPort: number, options?: CodexIntegrationInstallOptions) => unknown,
  then: (result: unknown) => unknown = (result) => result,
): unknown {
  void apiPort;
  void install;
  const path = codexHooksPath();
  if (!path || !host.fs.fileExists(path)) return then({ ok: true, changed: false });
  const json = host.fs.readJson<CodexHooks>(path);
  if (!json || !json.hooks) return then({ ok: true, changed: false });
  const owned = (json.hooks.SessionStart || []).filter(isCodetermCodexHook);
  if (owned.length === 0) return then({ ok: true, changed: false });
  return refreshCodexHookTrustState(path, (trustState) => {
    if (trustState === "disabled") {
      return then({ ok: true, changed: false, reason: "Codex hook trust is disabled" });
    }

    const scriptPath = codexWindowsHookScriptPath();
    const scriptStale =
      host.platform() === "windows" &&
      !!scriptPath &&
      (!host.fs.fileExists(scriptPath)
        || !isCurrentCodexHookScriptContent(String(host.fs.readFile(scriptPath) || "")));
    const commandStale = owned.some((entry) => {
      const hooks = (entry && entry.hooks) || [];
      return hooks.some(
        (hook) => {
          const command = String(hook && hook.command || "");
          return command.indexOf("$CODETERM_API_URL") === -1
            || command.indexOf("${CODETERM_API_TOKEN}") === -1
            || command.indexOf("$CODETERM_TAB_ID") === -1
            || command.indexOf("${CODETERM_SESSION_BINDING_NONCE}") === -1
            || command.indexOf("X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER) === -1
            || hasLiteralPort(command)
            || (host.platform() === "windows" && !hasCodetermCodexWindowsCommand(entry));
        },
      );
    });
    const stopEntries = json.hooks.Stop || [];
    const ownedStopEntries = stopEntries.filter(isCodetermCodexHook);
    const stopOwned = ownedStopEntries.length > 0;
    const stopCommandStale = host.platform() === "windows"
      && ownedStopEntries.some((entry) => !hasCodetermCodexWindowsCommand(entry));
    const timeoutStale = owned.some((entry) => !hasAdequateCodexHookTimeout(entry));
    const trustStale = trustState === "stale";
    if (!scriptStale && !commandStale && !timeoutStale && !stopCommandStale && stopOwned && !trustStale) {
      return then({ ok: true, changed: false });
    }

    return then({
      ok: false,
      changed: false,
      actionable: true,
      error: "Codex hook repair requires recorded integration consent; open provider integration settings and approve Repair",
    });
  });
}

function codexHooksPath(): string | null {
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "hooks.json";
}

function hasLiteralPort(value: string): boolean {
  return /:\d{2,5}/.test(value);
}

/// A CodeTerm-owned entry is only current when every one of its hooks grants the
/// cold-start-tolerant budget; the old 5s value is the bug this repairs.
function hookTimeoutsAdequate(
  sessionStart: CodexHookEntry[] | undefined,
  stop: CodexHookEntry[] | undefined,
): boolean {
  const owned = [...(sessionStart || []), ...(stop || [])].filter(isCodetermCodexHook);
  return owned.length > 0 && owned.every(hasAdequateCodexHookTimeout);
}

function codexConfigPath(): string | null {
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "config.toml";
}

function codexWindowsHookScriptPath(): string | null {
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "codeterm-session-hook.cmd";
}

function codexHookTrustOwnershipPath(): string | null {
  const config = codexConfigPath();
  return config ? config + ".codeterm-hook-trust-owned" : null;
}

function captureIntegrationFile(path: string): IntegrationFileSnapshot | null {
  const existed = host.fs.fileExists(path);
  const bytes = existed ? host.fs.readFile(path) : "";
  return typeof bytes === "string" ? { path, existed, bytes } : null;
}

function restoreIntegrationFiles(snapshots: IntegrationFileSnapshot[]): boolean {
  let restored = true;
  for (let i = snapshots.length - 1; i >= 0; i--) {
    const snapshot = snapshots[i];
    const ok = snapshot.existed
      ? host.fs.writeFile(snapshot.path, snapshot.bytes)
      : (!host.fs.fileExists(snapshot.path) || host.fs.removeFile(snapshot.path));
    restored = ok && restored;
  }
  return restored;
}

function codexHookTrustContext(hooksPath: string): {
  config: string;
  block: string;
  labels: string[];
  fingerprint: string;
  hookFingerprint: string;
  trustFingerprint: string;
} | null {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return null;
  const text = String(host.fs.readFile(cfg) || "");
  const block = findCodexHookTrustBlock(text, hooksPath);
  if (block == null) return null;
  const hooks = host.fs.readJson<CodexHooks>(hooksPath)?.hooks;
  const labels: string[] = [];
  if ((hooks?.SessionStart || []).some(isCodetermCodexHook)) labels.push("session_start");
  if ((hooks?.Stop || []).some(isCodetermCodexHook)) labels.push("stop");
  const script = codexWindowsHookScriptPath();
  const scriptText = script && host.fs.fileExists(script) ? String(host.fs.readFile(script) || "") : "";
  const trustFingerprint = codexHookContentHash(hookTrustConfigSections(text));
  const hookFingerprint = codexHookContentHash([
    ownedContentHash(hooks?.SessionStart, hooks?.Stop, scriptText),
    labels.join(","),
  ].join(String.fromCharCode(0)));
  const fingerprint = codexHookContentHash([trustFingerprint, hookFingerprint].join(String.fromCharCode(0)));
  return { config: text, block, labels, fingerprint, hookFingerprint, trustFingerprint };
}

function hookTrustConfigSections(config: string): string {
  const sections: string[] = [];
  let active = false;
  let lines: string[] = [];
  for (const line of config.split(/\r?\n/)) {
    const value = line.trim();
    const header = value.startsWith("[") && value.endsWith("]");
    if (header) {
      if (active) sections.push(lines.join("\n"));
      active = /^\[hooks\.state(?:\.|\])/.test(value);
      lines = active ? [line] : [];
    } else if (active) {
      lines.push(line);
    }
  }
  if (active) sections.push(lines.join("\n"));
  return sections.join(String.fromCharCode(0));
}

function cacheCodexHookTrust(hooksPath: string, verdict: CodexHookTrustVerdict): void {
  const context = codexHookTrustContext(hooksPath);
  if (context) codexHookTrustCache.set(hooksPath, {
    fingerprint: context.fingerprint,
    hookFingerprint: context.hookFingerprint,
    trustFingerprint: context.trustFingerprint,
    verdict,
  });
}

function codexHookTrustState(hooksPath: string): "enabled" | "disabled" | "missing" | "unknown" | "trusted" | "stale" {
  const context = codexHookTrustContext(hooksPath);
  if (!context) return "missing";
  const { block, fingerprint } = context;
  const m = block.match(/(?:^|\n)\s*enabled\s*=\s*(true|false)\s*(?:\n|$)/);
  if (m && m[1] === "false") return "disabled";
  if (/(?:^|\n)\s*trusted_hash\s*=/.test(block)) {
    if (!context.labels.includes("session_start")) return "unknown";
    const cached = codexHookTrustCache.get(hooksPath);
    if (cached?.fingerprint === fingerprint) return cached.verdict;
    if (cached?.verdict === "stale") return "stale";
    if (cached?.trustFingerprint === context.trustFingerprint
      && cached.hookFingerprint !== context.hookFingerprint) return "stale";
    return cached?.verdict || "trusted";
  }
  if (m) return "enabled";
  return "missing";
}

function refreshCodexHookTrustState(
  hooksPath: string,
  then: (verdict: "enabled" | "disabled" | "missing" | "unknown" | "trusted" | "stale") => unknown = (verdict) => verdict,
): unknown {
  const context = codexHookTrustContext(hooksPath);
  if (!context) return then("missing");
  if (/(?:^|\n)\s*enabled\s*=\s*false\s*(?:\n|$)/.test(context.block)) return then("disabled");
  if (!/(?:^|\n)\s*trusted_hash\s*=/.test(context.block)) return then(codexHookTrustState(hooksPath));
  const cached = codexHookTrustCache.get(hooksPath);
  if (cached?.fingerprint === context.fingerprint) return then(cached.verdict);
  const fallback = codexHookTrustState(hooksPath) === "stale" ? "stale" : "trusted";
  if (!context.labels.includes("session_start")) return then(fallback);
  return inspectCodexHookTrust({
    hooksPath,
    config: context.config,
    managedLabels: context.labels,
    listCwd: host.homeDir() || ".",
  }, (inspected) => {
    if (inspected !== "unknown") {
      codexHookTrustCache.set(hooksPath, {
        fingerprint: context.fingerprint,
        hookFingerprint: context.hookFingerprint,
        trustFingerprint: context.trustFingerprint,
        verdict: inspected,
      });
      return then(inspected);
    }
    return then(fallback);
  });
}

function enableExistingCodexHookTrust(
  hooksPath: string,
): { state: "enabled" | "missing" | "unknown" | "trusted"; changed: boolean; write_failed: boolean } {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return { state: "missing", changed: false, write_failed: false };
  const text = String(host.fs.readFile(cfg) || "");
  if (!text) return { state: "missing", changed: false, write_failed: false };
  const span = findCodexHookTrustBlockSpan(text, hooksPath);
  if (!span) return { state: "missing", changed: false, write_failed: false };
  const block = text.substring(span.start, span.end);
  if (!/(?:^|\n)\s*enabled\s*=\s*false\s*(?:\n|$)/.test(block)) {
    const current = codexHookTrustState(hooksPath);
    const state = current === "enabled" || current === "trusted" ? current : "unknown";
    return { state, changed: false, write_failed: false };
  }
  const nextBlock = block.replace(/(^|\n)(\s*enabled\s*=\s*)false(\s*(?:\n|$))/, "$1$2true$3");
  const next = text.substring(0, span.start) + nextBlock + text.substring(span.end);
  return host.fs.writeFile(cfg, next)
    ? { state: "enabled", changed: true, write_failed: false }
    : { state: "unknown", changed: false, write_failed: true };
}

function disableOwnedCodexHookTrust(hooksPath: string): boolean {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return true;
  const text = host.fs.readFile(cfg);
  if (typeof text !== "string") return false;
  const span = findCodexHookTrustBlockSpan(text, hooksPath);
  if (!span) return true;
  const block = text.substring(span.start, span.end);
  const nextBlock = block.replace(/(^|\n)(\s*enabled\s*=\s*)true(\s*(?:\n|$))/, "$1$2false$3");
  if (nextBlock === block) return true;
  const next = text.substring(0, span.start) + nextBlock + text.substring(span.end);
  return host.fs.writeFile(cfg, next);
}

function findCodexHookTrustBlock(text: string, hooksPath: string): string | null {
  const span = findCodexHookTrustBlockSpan(text, hooksPath);
  return span ? text.substring(span.start, span.end) : null;
}

function findCodexHookTrustBlockSpan(text: string, hooksPath: string): { start: number; end: number } | null {
  const lines = text.split("\n");
  const marker = normalizeCodexHookKey(hooksPath + ":session_start:");
  let offset = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trim();
    const rawKey = hookTrustKeyFromHeader(trimmed);
    if (rawKey !== null && normalizeCodexHookKey(rawKey).startsWith(marker)) {
      const start = offset;
      let end = text.length;
      let scanOffset = offset + line.length + 1;
      for (let j = i + 1; j < lines.length; j++) {
        const t = lines[j].trim();
        if (t.charAt(0) === "[" && t.charAt(t.length - 1) === "]") {
          end = scanOffset;
          break;
        }
        scanOffset += lines[j].length + 1;
      }
      return { start, end };
    }
    offset += line.length + 1;
  }
  return null;
}

function hookTrustKeyFromHeader(header: string): string | null {
  const prefix = "[hooks.state.";
  if (!header.startsWith(prefix) || !header.endsWith("]")) return null;
  const raw = header.slice(prefix.length, -1).trim();
  if (raw.startsWith("'") && raw.endsWith("'")) return raw.slice(1, -1);
  if (raw.startsWith('"') && raw.endsWith('"')) {
    try { return JSON.parse(raw); } catch { return null; }
  }
  return null;
}

function normalizeCodexHookKey(value: string): string {
  const normalized = value.replace(/\\/g, "/");
  return host.platform() === "windows" ? normalized.toLowerCase() : normalized;
}

function hasCodetermCodexWindowsCommand(entry: CodexHookEntry): boolean {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (let i = 0; i < hooks.length; i++) {
    const hook = hooks[i];
    if (!hook) continue;
    const command = String(hook.commandWindows || "");
    const fullCommand = String(hook.command || "") + "\n" + command;
    const owned = fullCommand.indexOf("X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER) !== -1
      || fullCommand.indexOf("X-CodeTerm-Hook:" + CODETERM_CODEX_HOOK_MARKER) !== -1
      || hook._source === CODETERM_CODEX_HOOK_MARKER;
    if (
      owned
      && command.startsWith('cmd.exe /d /s /c call "')
      && command.indexOf("codeterm-session-hook.cmd") !== -1
    ) {
      return true;
    }
  }
  return false;
}
