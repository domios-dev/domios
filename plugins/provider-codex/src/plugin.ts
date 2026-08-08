import type { LaunchParams, ModelInfo, ParsedPrompt, PluginModule } from "@codeterm/plugin-sdk";
import { discoverCodexModels } from "./models";
import {
  parseCodexPrompt,
  responseKeysForIndex,
} from "./prompt";
import { readSessionMeta, SESSION_MODEL_TAIL_BYTES } from "./sessionMeta";
import { fetchCodexUsage, parseCodexUsage } from "./usage";
import { summarizeExecScript, unwrapShellCommand } from "./execWrapper";
import { patchEdits } from "./patchEdits";
import {
  classifyCodexTabState,
  type TabStateClassification,
  type TabStateSnapshot,
} from "./tabState";
import { grantCodexHookTrust } from "./appServer";

const CODETERM_CODEX_HOOK_MARKER = "codeterm-codex-session-binding-v1";
const CODETERM_CODEX_PROFILE_CLEANUP_MARKER = "codeterm-profile-cleanup-v1";
const TITLE_RE = /codex/i;
const OUTPUT_FINGERPRINTS = [
  "OpenAI Codex",
  "permissions: YOLO mode",
  "permissions: read-only",
  "Would you like to run the following command",
  "Would you like to make the following edits",
  "approve network access",
  "Would you like to grant these permissions",
  "codex>",
];
const TUI_FRAGMENTS = ["openai codex", "ctrl+c to cancel"];
const ASSIGNED_TASK_START_PROMPT = "Execute the assigned task. Follow the provided developer instructions.";
const IDLE_WAKEUP_START_PROMPT = "You are a newly started agent. Wait for instructions from the user or your orchestration parent.";

function starterPrompt(params: LaunchParams): string | undefined {
  if (params.starterPrompt === "no_starter") return undefined;
  if (params.starterPrompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (params.starterPrompt === "team_bootstrap") {
    if (!params.starterPromptText) throw new Error("team_bootstrap requires host starterPromptText");
    return params.starterPromptText;
  }
  return ASSIGNED_TASK_START_PROMPT;
}

function hasPromptAndStatusFooter(screen: string): boolean {
  if (screen.indexOf("›") === -1) return false;
  return screen.split(/\r?\n/).some((line) => {
    const separator = line.indexOf(" · ");
    if (separator === -1) return false;
    return /(?:[a-z]:\\|\/)/i.test(line.slice(separator + 3));
  });
}

const plugin: PluginModule = {
  starterPromptText(prompt: "no_starter" | "assigned_task" | "idle_wakeup" | "team_bootstrap"): string | undefined {
    return prompt === "team_bootstrap" ? undefined : starterPrompt({ starterPrompt: prompt });
  },
  detectFromTitle(title: string): boolean {
    return TITLE_RE.test(String(title || ""));
  },

  detectFromOutput(text: string): boolean {
    const t = String(text || "");
    for (let i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
      if (t.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) return true;
    }
    return false;
  },

  screenHasTui(screen: string): boolean {
    const text = String(screen || "");
    const lower = text.toLowerCase();
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    return hasPromptAndStatusFooter(text);
  },

  isOutputNoise(text: string): boolean {
    const trimmed = String(text || "").trim();
    if (!trimmed) return true;
    for (let i = 0; i < trimmed.length; i++) {
      const c = trimmed[i];
      if (
        c !== "─" && c !== "━" && c !== "═" &&
        c !== " " && c !== "\t" && c !== "\n" && c !== "\r"
      ) {
        return false;
      }
    }
    return true;
  },

  hasEventMarkers(chunk: string): boolean {
    const c = String(chunk || "");
    return (
      c.indexOf("Would you like") !== -1 ||
      c.indexOf("approve") !== -1 ||
      c.indexOf("run the following") !== -1 ||
      c.indexOf("make the following") !== -1 ||
      c.indexOf("grant these permissions") !== -1 ||
      // screen off the auto-approve path; the universal numbered parser then extracts.
      c.indexOf("to confirm") !== -1 ||
      c.indexOf("esc to go back") !== -1 ||
      c.indexOf("Select Model") !== -1 ||
      c.indexOf("Select Reasoning") !== -1 ||
      c.indexOf("›") !== -1 ||
      c.indexOf("❯") !== -1
    );
  },

  detectEvents(text: string): string[] {
    const t = String(text || "");
    const events: string[] = [];
    if (
      t.indexOf("Would you like to run the following command") !== -1 ||
      t.indexOf("Would you like to make the following edits") !== -1 ||
      t.indexOf("Do you want to approve network access") !== -1 ||
      t.indexOf("Would you like to grant these permissions") !== -1
    ) {
      events.push("permission_request");
    }
    return events;
  },

  parsePrompt(text: string): ParsedPrompt | null {
    return parseCodexPrompt(String(text || ""));
  },

  buildPromptResponse(optionIndex: number, screen?: string): string[] {
    return responseKeysForIndex(optionIndex, String(screen || ""));
  },

  directoryTrustPromptResponse(prompt: ParsedPrompt, screen?: string): string[] | null {
    const question = String(prompt.question || "").toLowerCase();
    const first = prompt.options[0];
    const firstLabel = Array.isArray(first) ? first[1] : (first as unknown as { label?: string }).label;
    if (!question.includes("trust the contents of this directory") || !firstLabel || !/^yes\b/i.test(firstLabel)) {
      return null;
    }
    return responseKeysForIndex(0, String(screen || ""));
  },

  // Codex CLI has no model-list subcommand; its local cache is the only sync account-scoped source. Missing => seed.
  discoverModels(): ModelInfo[] {
    return discoverCodexModels();
  },

  buildLaunchCommand(params: LaunchParams): string {
    const p = params || {};
    const parts: string[] = [];
    let prefix = "";
    parts.push("codex");
    if (p.skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
    }
    if (p.args && p.args.length > 0) {
      // Quote each caller-supplied arg to prevent shell injection.
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    if (p.task?.trim()) {
      const taskPath = host.fs.writeTempFile(p.task, ".ct-task.md");
      if (!taskPath) throw new Error("failed to write Codex task instructions");
      const quotedPath = host.shell.quoteFor(taskPath, host.platform());
      if (host.platform() === "windows") {
        prefix = "Add-Type -AssemblyName System.Web.Extensions; "
          + `$ctTaskInstructions = Get-Content -Raw -Encoding UTF8 ${quotedPath}; `
          + "$ctTaskJson = (New-Object System.Web.Script.Serialization.JavaScriptSerializer).Serialize($ctTaskInstructions); "
          + "$ctProfileSuffix = if ($env:CODETERM_TAB_ID) { $env:CODETERM_TAB_ID -replace '[^A-Za-z0-9_-]', '-' } else { [guid]::NewGuid().ToString('N') }; "
          + "$ctProfileName = 'codeterm-' + $ctProfileSuffix; "
          + "$ctProfilePath = Join-Path $env:USERPROFILE ('.codex\\' + $ctProfileName + '.config.toml'); "
          + "Set-Content -LiteralPath $ctProfilePath -Encoding UTF8 -Value ('developer_instructions = ' + $ctTaskJson); "
          + `Remove-Item -LiteralPath ${quotedPath} -Force -ErrorAction SilentlyContinue; try { `;
        parts.push("--profile");
        parts.push("$ctProfileName");
      } else {
        parts.push("-c");
        parts.push(`"developer_instructions=$(cat ${quotedPath})"`);
      }
      const prompt = starterPrompt(p);
      if (prompt) {
        parts.push("--");
        parts.push(host.shell.quoteFor(prompt, host.platform()));
      }
      if (host.platform() === "windows") {
        return prefix + parts.join(" ") + " } finally { Remove-Item -LiteralPath $ctProfilePath -Force -ErrorAction SilentlyContinue }";
      }
    }
    return prefix + parts.join(" ");
  },

  buildResumeCommand(sessionId: string, skipPermissions?: boolean): string {
    const parts = ["codex", "resume"];
    if (skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
    }
    // session_id accepts UUID or rollout filename stem.
    const uuid = extractSessionUuid(sessionId) || sessionId;
    parts.push(host.shell.quoteFor(uuid, host.platform()));
    return parts.join(" ");
  },

  // Rollout-tree detection: scan `~/.codex/sessions/.../rollout-*.jsonl`, reading only `session_meta` headers.
  sessionFilePath(_cwd: string, sessionId: string): string | null {
    const root = codexSessionsRoot();
    if (!root) return null;
    let hit: string | null = null;
    const files = host.fs.walkDir(root, 5);
    for (let i = 0; i < files.length; i++) {
      const f = files[i];
      if (!f.isFile || !endsWith(f.name, ".jsonl")) continue;
      const stem = f.name.substring(0, f.name.length - 6);
      // The rollout's own session_meta id is provider-owned evidence and wins
      // over filename reconstruction; files without one keep stem matching.
      const meta = readSessionMeta(f.path);
      const storedId = meta && meta.sessionId;
      const matched = storedId
        ? storedId === sessionId
        : stem === sessionId || extractSessionUuid(stem) === sessionId;
      if (matched) {
        hit = f.path;
        break;
      }
    }
    return hit;
  },

  // Chat uses generic JSONL-session hook; codex paths resolve via sessionFilePath.
  sessionJsonlPath(cwd: string, sessionId: string): string | null {
    return this.sessionFilePath!(cwd, sessionId);
  },

  // Duplicated by design across provider bundles (QuickJS, no imports) — parity guarded by provider-helper-parity.test.ts.
  parseSessionDelta(chunk: string): unknown {
    const out: unknown[] = [];
    const lines = String(chunk || "").split("\n");
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
      if (!lines[i]) continue;
      let v: CodexEntry;
      try { v = JSON.parse(lines[i]); } catch (_) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!validateEntry(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      let ts: number | null = null;
      const tsIso: string | null = (typeof v.timestamp === "string") ? v.timestamp : null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1000);
      }
      if (v.type === "event_msg" && v.payload && v.payload.type === "patch_apply_end") {
        const p = v.payload;
        const content = typeof p.stdout === "string" ? p.stdout : "";
        const callId = typeof p.call_id === "string" ? p.call_id : "";
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_result", data: { content: content, tool_use_id: callId } }],
          ts: ts,
          timestamp: tsIso,
        });
        continue;
      }
      if (!v || v.type !== "response_item" || !v.payload) continue;
      const p = v.payload;
      if (p.type === "message") {
        const role = p.role === "assistant" ? "assistant" : (p.role === "user" ? "user" : null);
        if (!role) continue; // skip developer/system noise
        let text = "";
        if (Array.isArray(p.content)) {
          for (let j = 0; j < p.content.length; j++) {
            const c = p.content[j];
            if (c && typeof c.text === "string") text += c.text;
          }
        } else if (typeof p.content === "string") {
          text = p.content;
        }
        if (!text) continue;
        const systemSubtype = classifyCodexSystemText(text);
        if (systemSubtype) {
          out.push({
            role: "system",
            blocks: [{ kind: "text", data: { text: text, subtype: systemSubtype } }],
            sourceType: v.type,
            sourceSubtype: systemSubtype,
            ts: ts,
            timestamp: tsIso,
          });
          continue;
        }
        out.push({ role: role, blocks: [{ kind: "text", data: { text: text } }], ts: ts, timestamp: tsIso });
      } else if (p.type === "function_call") {
        let input: Record<string, unknown> = {};
        try { input = JSON.parse(p.arguments || "{}"); } catch (_) { input = { raw: p.arguments }; }
        if (input && input.cmd && !input.command) input.command = input.cmd; // surface for generic readers
        const name = typeof p.name === "string" ? p.name.trim() : "";
        if (isInternalOrchestrationCall(name) || !hasUsefulFunctionCall(name, input, p.arguments)) continue;
        const callId = typeof p.call_id === "string" ? p.call_id : "";
        const canonical = canonicalToolFields(name, input);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_use", data: { name: name || "tool", id: callId, input: input, ...canonical } }],
          ts: ts,
          timestamp: tsIso,
        });
      } else if (p.type === "function_call_output" || p.type === "custom_tool_call_output") {
        const callId = typeof p.call_id === "string" ? p.call_id : "";
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_result", data: { content: codexToolOutputText(p.output), tool_use_id: callId } }],
          ts: ts,
          timestamp: tsIso,
        });
      } else if (p.type === "custom_tool_call") {
        let name = typeof p.name === "string" ? p.name.trim() : "";
        if (!name) continue;
        if (isInternalOrchestrationCall(name)) continue;
        let input: Record<string, unknown> = {};
        if (name === "apply_patch" && typeof p.input === "string") {
          input = { patch: p.input };
        } else if (typeof p.input === "string") {
          try { input = JSON.parse(p.input); } catch (_) { input = { raw: p.input }; }
        } else if (p.input && typeof p.input === "object" && !Array.isArray(p.input)) {
          input = p.input as Record<string, unknown>;
        }
        const nestedPatch = name === "exec" && typeof input.raw === "string"
          ? nestedApplyPatch(input.raw)
          : null;
        if (nestedPatch) {
          name = "apply_patch";
          input = { patch: nestedPatch };
        }
        const callId = typeof p.call_id === "string" ? p.call_id : "";
        const canonical = canonicalToolFields(name, input);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_use", data: { name: name, id: callId, input: input, ...canonical } }],
          ts: ts,
          timestamp: tsIso,
        });
      }
    }
    return { messages: out };
  },

  detectSessionId(cwd: string, exclude?: string[] | string, maxAgeMs?: number): string | null {
    const root = codexSessionsRoot();
    if (!root || !host.fs.fileExists(root)) return null;
    const excluded = new Set(Array.isArray(exclude) ? exclude : (exclude ? [exclude] : []));
    const now = host.unixNowMs ? host.unixNowMs() : Date.now();
    const ageLimit = Number.isFinite(maxAgeMs) && (maxAgeMs as number) > 0 ? (maxAgeMs as number) : 300000;
    let best: { sid: string; modifiedMs: number } | null = null;
    const files = host.fs.walkDir(root, 5);
    for (let i = 0; i < files.length; i++) {
      const f = files[i];
      if (!f.isFile || !endsWith(f.name, ".jsonl")) continue;
      const modifiedMs = f.modifiedMs || 0;
      if (modifiedMs > 0 && now - modifiedMs > ageLimit) continue;
      const stem = f.name.substring(0, f.name.length - 6);
      const meta = readSessionMeta(f.path);
      const sid = (meta && meta.sessionId) || extractSessionUuid(stem);
      if (!sid || excluded.has(sid) || excluded.has(stem)) continue;
      const project = meta && meta.cwd;
      if (cwd && project && !host.path.equal(project, cwd)) continue;
      if (!best || modifiedMs > best.modifiedMs) best = { sid, modifiedMs };
    }
    return best ? best.sid : null;
  },

  checkIntegration(): unknown {
    const path = codexHooksPath();
    if (!path || !host.fs.fileExists(path)) {
      return { hooks_file_exists: false, hooks_installed: false, session_start_hook: false, hook_trust_state: "missing", hook_trust_enabled: null, target: path || "~/.codex/hooks.json", backup_exists: !!path && host.fs.fileExists(path + ".codeterm-backup"), proposed_changes: ["SessionStart hook", "hook trust"] };
    }
    const json = host.fs.readJson<CodexHooks>(path);
    if (!json) {
      return { hooks_file_exists: true, hooks_installed: false, session_start_hook: false, hook_trust_state: "missing", hook_trust_enabled: null };
    }
    const arr = json.hooks && json.hooks.SessionStart;
    const installed = Array.isArray(arr) && arr.some(isCodetermCodexHook);
    const winScript = codexWindowsHookScriptPath();
    const winScriptText = winScript && host.fs.fileExists(winScript)
      ? String(host.fs.readFile(winScript) || "")
      : "";
    const windowsReady = host.platform() !== "windows" || (
      !!winScript &&
      winScriptText.indexOf(CODETERM_CODEX_PROFILE_CLEANUP_MARKER) !== -1 &&
      Array.isArray(arr) &&
      arr.some(hasCodetermCodexWindowsCommand)
    );
    const contractOk = host.platform() === "windows"
      ? winScriptText.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1 &&
        winScriptText.indexOf("CODETERM_TAB_ID") !== -1 &&
        winScriptText.indexOf("/api/tabs/") !== -1
      : (Array.isArray(arr) && arr.some((entry) => {
          const hk = entry && entry.hooks;
          return Array.isArray(hk) && hk.some((h) => {
            const command = String(h?.command || "");
            return command.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1 &&
              command.indexOf("CODETERM_TAB_ID") !== -1 &&
              command.indexOf("/api/tabs/") !== -1;
          });
        }));
    const current = installed && windowsReady && contractOk;
    const trustState = codexHookTrustState(path);
    const trustOk = trustState !== "disabled";
    return {
      hooks_file_exists: true,
      hooks_installed: current && trustOk,
      session_start_hook: current && trustOk,
      hook_trust_state: trustState,
      hook_trust_enabled: trustState === "enabled" ? true : (trustState === "disabled" ? false : null),
      target: path,
      backup_exists: host.fs.fileExists(path + ".codeterm-backup"),
      proposed_changes: ["SessionStart hook", "hook trust"],
    };
  },

  installIntegration(
    apiPort: number,
    options?: { consent?: boolean },
  ): unknown {
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
    if (json.hooks.SessionStart !== undefined && !Array.isArray(json.hooks.SessionStart)) {
      return { ok: false, error: "hooks.SessionStart is not an array" };
    }
    if (!json.hooks.SessionStart) json.hooks.SessionStart = [];
    json.hooks.SessionStart = json.hooks.SessionStart.filter((entry) => !isCodetermCodexHook(entry));

    const nullDev = host.platform() === "windows" ? "NUL" : "/dev/null";
    const port = apiPort | 0;
    const authH = '-H "Authorization: Bearer ${CODETERM_API_TOKEN}"';
    const nonceH = '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"';
    const guard = 'if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then ';
    const cmd = guard + 'curl -s -X POST "$CODETERM_API_URL'
      + "/api/tabs/$CODETERM_TAB_ID/hook/session_start\" "
      + "-H 'Content-Type: application/json' "
      + "-H 'X-CodeTerm-Agent-Provider: codex' "
      + nonceH + " "
      + authH
      + " --data-binary @- > " + nullDev + " 2>&1 || true; fi";
    const commandWindows = scriptPath
      ? 'powershell -NoProfile -ExecutionPolicy Bypass -File "' + scriptPath + '"'
      : cmd;

    if (scriptPath && !host.fs.writeFile(scriptPath, codexWindowsHookScript(port))) {
      return fail("writeFile failed for " + scriptPath);
    }

    json.hooks.SessionStart.push({
      matcher: "startup|resume|clear|compact",
      hooks: [{
        type: "command",
        command: cmd,
        commandWindows: commandWindows,
        timeout: 5,
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
      host.log("info", "[codex-plugin] SessionStart hook installed at " + path);
      return { ok: true, hook_trust_state: trust.state };
    }
    return fail("writeJson failed for " + path);
  },

  // integration.trust_grant capability: core invokes this once per install
  // with the manifest-declared managed hook key(s). Codex is the only hash
  // authority — the grant lane never computes a trust hash CodeTerm-side.
  grantIntegrationTrust(managedKeys: string[]): unknown {
    const hooksPath = codexHooksPath();
    if (!hooksPath) {
      return { granted: false, wrote: false, degraded: true, verify_failed: false, reason: "could not resolve ~/.codex/hooks.json" };
    }
    return grantCodexHookTrust({
      hooksPath: hooksPath,
      configPath: codexConfigPath(),
      managedLabels: Array.isArray(managedKeys) ? managedKeys.map(String) : [],
      listCwd: host.homeDir() || ".",
    });
  },

  uninstallIntegration(): unknown {
    const path = codexHooksPath();
    if (!path) return { ok: true };
    let hooksOk = true;
    if (host.fs.fileExists(path)) {
      const json = host.fs.readJson<CodexHooks>(path);
      if (!json || !json.hooks || typeof json.hooks !== "object") {
        return { ok: false, error: "could not safely read hooks JSON at " + path };
      }
      if (Array.isArray(json.hooks.SessionStart)) {
        json.hooks.SessionStart = json.hooks.SessionStart.filter((entry) => !isCodetermCodexHook(entry));
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
  },

  restoreIntegration(): unknown {
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
  },

  ensureIntegrationOnStartup(apiPort: number): unknown {
    void apiPort;
    return { ok: true, changed: false };
  },

  // Parse the `/status` TUI screen into a UsageSnapshot; gauges report "% left", snapshot stores "% used".
  parseUsage(rawText: string, nowMs: number): unknown {
    return parseCodexUsage(rawText, nowMs);
  },

  fetchUsage(): unknown {
    return fetchCodexUsage(host);
  },

  detectActiveSession(_cwd: string, _currentSid?: string): string | null {
    return null;
  },

  enumerateSessions(): unknown[] {
    const root = codexSessionsRoot();
    if (!root || !host.fs.fileExists(root)) return [];
    const out: unknown[] = [];
    const files = host.fs.walkDir(root, 5);
    for (let i = 0; i < files.length; i++) {
      const f = files[i];
      if (!f.isFile || !endsWith(f.name, ".jsonl")) continue;
      const stem = f.name.substring(0, f.name.length - 6);
      const meta = readSessionMeta(f.path);
      const sid = (meta && meta.sessionId) || extractSessionUuid(stem);
      if (!sid) continue;
      out.push({
        sessionId: sid,
        provider: "codex",
        projectPath: meta && meta.cwd ? meta.cwd : null,
        title: null,
        summary: null,
        filePath: f.path,
        createdTs: f.createdMs ? Math.floor(f.createdMs / 1000) : null,
        modifiedTs: f.modifiedMs ? Math.floor(f.modifiedMs / 1000) : null,
      });
    }
    return out;
  },

  // Read the exact model identity from this session's latest turn context.
  detectSessionModel(cwd: string, sessionId: string): string | null {
    const path = this.sessionFilePath!(cwd, sessionId);
    if (!path) return null;
    // Bounded tail read: a partial leading line just fails JSON.parse below and is skipped.
    const contents = host.fs.readFileTail(path, SESSION_MODEL_TAIL_BYTES);
    if (!contents) return null;
    const lines = contents.split("\n");
    for (let i = lines.length - 1; i >= 0; i--) {
      const line = lines[i];
      if (!line) continue;
      let v: CodexEntry;
      try { v = JSON.parse(line); } catch (_) { continue; }
      if (!v || v.type !== "turn_context") continue;
      const m = v.payload && v.payload.model;
      if (typeof m === "string" && m.trim()) return m.trim();
    }
    return null;
  },

  // Mirror native parse_session_messages for response_item message blocks.
  parseSessionMessages(text: string): unknown[] {
    const out: unknown[] = [];
    const lines = String(text || "").split("\n");
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      if (!line) continue;
      let v: CodexEntry;
      try { v = JSON.parse(line); } catch (e) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!isValidCodexSessionMessage(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!v || v.type !== "response_item") continue;
      const payload = v.payload;
      if (!payload || payload.type !== "message") continue;
      const role = payload.role;
      if (role !== "user" && role !== "assistant") continue;
      const content = payload.content;
      if (!Array.isArray(content)) continue;
      const chunks: string[] = [];
      for (let j = 0; j < content.length; j++) {
        const b = content[j];
        if (!b) continue;
        if (
          (b.type === "input_text" ||
            b.type === "output_text" ||
            b.type === "text") &&
          typeof b.text === "string"
        ) {
          chunks.push(b.text);
        }
      }
      const text2 = chunks.join(" ");
      if (!text2.trim()) continue;
      let ts: number | null = null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1000);
      }
      const systemSubtype = classifyCodexSystemText(text2);
      if (systemSubtype) {
        out.push({ role: "system", text: text2, ts: ts, isMeta: true, sourceSubtype: systemSubtype });
      } else {
        out.push({ role: role, text: text2, ts: ts, isMeta: false });
      }
    }
    return out;
  },

  classifyTabState(snapshot: TabStateSnapshot): TabStateClassification | null {
    return classifyCodexTabState(snapshot, host.unixNowMs());
  },

  providerSelfUpdateCapability(): {
    inSession: boolean;
    restartRequired: boolean;
    completionSignal: "provider_classification";
  } {
    return {
      inSession: true,
      restartRequired: true,
      completionSignal: "provider_classification",
    };
  },
};

// types shared by the parsers above

interface CodexContentBlock { type?: string; text?: string }
interface CodexPayload {
  type?: string;
  role?: string;
  content?: string | CodexContentBlock[];
  arguments?: string;
  name?: string;
  output?: unknown;
  model?: string;
  input?: string | Record<string, unknown>;
  call_id?: string;
  stdout?: string;
  success?: boolean;
}
interface CodexEntry { type?: string; payload?: CodexPayload; timestamp?: string }
interface CodexHookCmd { type?: string; command?: string; commandWindows?: string; timeout?: number; _source?: string }

function nestedApplyPatch(raw: string): string | null {
  const call = /\btools\.apply_patch\(\s*([A-Za-z_$][\w$]*|"(?:\\.|[^"\\])*")\s*\)/.exec(raw);
  if (!call) return null;
  let literal = call[1];
  if (literal.charAt(0) !== '"') {
    const escaped = literal.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const declaration = new RegExp(`\\b(?:const|let)\\s+${escaped}\\s*=\\s*("(?:\\\\.|[^"\\\\])*")\\s*;`).exec(raw);
    if (!declaration) return null;
    literal = declaration[1];
  }
  try {
    const patch = JSON.parse(literal);
    return typeof patch === "string" && patch.includes("*** Begin Patch") && patch.includes("*** End Patch")
      ? patch
      : null;
  } catch (_) {
    return null;
  }
}
interface CodexHookEntry { matcher?: string; hooks?: CodexHookCmd[] }
interface CodexHooks { hooks?: { SessionStart?: CodexHookEntry[] } }

function canonicalToolFields(name: string, input: Record<string, unknown>): Record<string, unknown> {
  const raw = typeof input.raw === "string" ? input.raw : "";
  const command = typeof input.command === "string" ? input.command : typeof input.cmd === "string" ? input.cmd : "";
  const script = name === "exec" && raw ? summarizeExecScript(raw) : null;
  const shell = raw ? unwrapShellCommand(raw) : null;
  const patch = typeof input.patch === "string" ? input.patch : name === "apply_patch" && typeof input.input === "string" ? input.input : "";
  const edits = patch ? patchEdits(patch) : [];
  if (script) return { tool_kind: script.kind, tool_summary: script.summary, tool_detail: script.detail };
  if (shell) return { tool_kind: "command", tool_summary: shell.command, tool_detail: shell.command };
  if (command.trim()) return { tool_kind: "command", tool_summary: command.trim(), tool_detail: command.trim() };
  if (edits.length > 0) return { tool_kind: "edit", tool_summary: edits[0].path, tool_detail: patch, tool_edits: edits };
  if (patch.indexOf("*** Begin Patch") >= 0) return { tool_kind: "edit", tool_summary: "Apply patch", tool_detail: patch, tool_edits: [] };
  return { tool_kind: "generic", tool_summary: name || "Tool call", tool_detail: name || "Tool call" };
}

// Use function declarations for QuickJS strict-mode compatibility with bundler rewrites.
function endsWith(s: string, suffix: string): boolean {
  return s.length >= suffix.length
    && s.substring(s.length - suffix.length) === suffix;
}

function codexSessionsRoot(): string | null {
  const override = host.envGet("CODETERM_CODEX_SESSIONS_ROOT");
  if (override) return override;
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "sessions";
}

function codexHooksPath(): string | null {
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "hooks.json";
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
  return home + sep + ".codex" + sep + "codeterm-session-start.ps1";
}

function codexHookTrustOwnershipPath(): string | null {
  const config = codexConfigPath();
  return config ? config + ".codeterm-hook-trust-owned" : null;
}

interface IntegrationFileSnapshot {
  path: string;
  existed: boolean;
  bytes: string;
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

function codexWindowsHookScript(_apiPort: number): string {
  return [
    "# " + CODETERM_CODEX_HOOK_MARKER,
    "# " + CODETERM_CODEX_PROFILE_CLEANUP_MARKER,
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
    'Invoke-RestMethod -Method Post -Uri "$base/api/tabs/$tab/hook/session_start" -Headers $headers -Body $body -TimeoutSec 5 | Out-Null',
    "$profileSuffix = $tab -replace '[^A-Za-z0-9_-]', '-'",
    '$profilePath = Join-Path $env:USERPROFILE (".codex\\codeterm-$profileSuffix.config.toml")',
    "Remove-Item -LiteralPath $profilePath -Force -ErrorAction SilentlyContinue",
    "exit 0",
    "",
  ].join("\n");
}

function isCodetermCodexHook(entry: CodexHookEntry): boolean {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (let i = 0; i < hooks.length; i++) {
    const hook = hooks[i];
    if (!hook) continue;
    if (hook._source === CODETERM_CODEX_HOOK_MARKER) return true;
    const cmd = String(hook.command || "");
    if (
      cmd.indexOf("/hook/session_start") !== -1 &&
      cmd.indexOf("X-CodeTerm-Agent-Provider: codex") !== -1
    ) {
      return true;
    }
  }
  return false;
}

function isOwnedCodexWindowsHookScript(path: string): boolean {
  const text = host.fs.readFile(path);
  return typeof text === "string" && text === codexWindowsHookScript(0);
}

function codexHookTrustState(hooksPath: string): "enabled" | "disabled" | "missing" | "unknown" | "trusted" {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return "missing";
  const text = String(host.fs.readFile(cfg) || "");
  if (!text) return "missing";
  const block = findCodexHookTrustBlock(text, hooksPath);
  if (block == null) return "missing";
  const m = block.match(/(?:^|\n)\s*enabled\s*=\s*(true|false)\s*(?:\n|$)/);
  if (m) return m[1] === "true" ? "enabled" : "disabled";
  // Codex >=0.129 per-hook trust: app-server config/batchWrite stores a
  // Codex-computed trusted_hash without an enabled key.
  if (/(?:^|\n)\s*trusted_hash\s*=/.test(block)) return "trusted";
  return "unknown";
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
  const marker = hooksPath + ":session_start:";
  const lines = text.split("\n");
  let offset = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trim();
    const isHeader = trimmed.charAt(0) === "[" && trimmed.charAt(trimmed.length - 1) === "]";
    if (isHeader && trimmed.indexOf("hooks.state.") !== -1 && trimmed.indexOf(marker) !== -1) {
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
      return { start: start, end: end };
    }
    offset += line.length + 1;
  }
  return null;
}

function hasCodetermCodexWindowsCommand(entry: CodexHookEntry): boolean {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (let i = 0; i < hooks.length; i++) {
    const hook = hooks[i];
    if (!hook || hook._source !== CODETERM_CODEX_HOOK_MARKER) continue;
    const command = String(hook.commandWindows || "");
    if (command.indexOf("codeterm-session-start.ps1") !== -1) {
      return true;
    }
  }
  return false;
}

function extractSessionUuid(stem: string): string | null {
  if (typeof stem !== "string") return null;
  const prefix = "rollout-";
  if (stem.indexOf(prefix) !== 0) return null;
  const rest = stem.substring(prefix.length);
  if (rest.length <= 20) return null;
  if (rest.charAt(19) !== "-") return null;
  return rest.substring(20);
}

function isValidCodexSessionDelta(v: CodexEntry): boolean {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type === "event_msg") {
    if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
    return v.payload.type === "patch_apply_end";
  }
  if (v.type !== "response_item") return false;
  if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
  const kind = v.payload.type;
  if (
    kind !== "message" &&
    kind !== "function_call" &&
    kind !== "function_call_output" &&
    kind !== "custom_tool_call" &&
    kind !== "custom_tool_call_output"
  ) {
    return false;
  }
  return true;
}

function codexToolOutputText(output: unknown): string {
  if (typeof output === "string") return output;
  if (Array.isArray(output)) {
    const text: string[] = [];
    for (let i = 0; i < output.length; i++) {
      const item = output[i];
      if (item && typeof item === "object" && typeof (item as { text?: unknown }).text === "string") {
        text.push((item as { text: string }).text);
      }
    }
    if (text.length > 0) return text.join("");
  }
  if (output == null) return "";
  return typeof output === "object" ? JSON.stringify(output) : String(output);
}

function validateEntry(v: CodexEntry): boolean {
  return isValidCodexSessionDelta(v);
}

function hasUsefulFunctionCall(name: string, input: Record<string, unknown>, args: string | undefined): boolean {
  if (name) return true;
  if (typeof args === "string" && args.trim() && args.trim() !== "{}") return true;
  return Object.keys(input || {}).length > 0;
}

function isInternalOrchestrationCall(name: string): boolean {
  return name === "send_message"
    || name === "spawn_agent"
    || name === "followup_task"
    || name === "interrupt_agent"
    || name === "list_agents"
    || name === "wait_agent";
}

function classifyCodexSystemText(text: string): string | null {
  const trimmed = String(text || "").trim();
  if (!trimmed) return null;
  if (
    trimmed.indexOf("<turn_aborted>") === 0 &&
    trimmed.indexOf("</turn_aborted>") !== -1
  ) {
    return "turn_aborted";
  }
  if (
    /^# AGENTS\.md instructions for /m.test(trimmed) &&
    trimmed.indexOf("<INSTRUCTIONS>") !== -1
  ) {
    return "agent_instructions";
  }
  if (
    /^<environment_context(?:\s[^>]*)?>[\s\S]*<\/environment_context>$/.test(trimmed)
  ) {
    return "environment_context";
  }
  return null;
}

function isValidCodexSessionMessage(v: CodexEntry): boolean {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type !== "response_item") return false;
  if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
  if (v.payload.type !== "message") return false;
  if (v.payload.role !== "user" && v.payload.role !== "assistant") return false;
  if (!Array.isArray(v.payload.content)) return false;
  return true;
}

export default plugin;
