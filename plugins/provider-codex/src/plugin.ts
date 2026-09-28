import type {
  LaunchParams,
  LaunchSessionDetection,
  LaunchSessionEvidence,
  ModelInfo,
  ParsedPrompt,
  PluginModule,
  ResumeParams,
  SessionDeltaContext,
  TabState,
} from "@codeterm/plugin-sdk";
import { discoverCodexModels } from "./models";
import { detectCodexLaunchSession, withLaunchMarker } from "./launchSession";
import {
  classifyCodexFolderTrustPrompt,
  parseMenuOptions,
  parseCodexFolderTrustPrompt,
  parseCodexPrompt,
  responseKeysForIndex,
} from "./prompt";
import { fetchCodexUsage, parseCodexUsage } from "./usage";
import { execScriptCommands, summarizeExecScript, unwrapCommandExecution, unwrapShellCommand } from "./execWrapper";
import { patchEdits } from "./patchEdits";
import { completedActivity } from "./activity";
import { agentMessageRow, reasoningResponseRow, subAgentRow } from "./agentActivity";
import { codexFileDiffs } from "./fileDiffs";
import { imageGenerationEndResult, webSearchEndResult } from "./richResults";
import { actionTitle } from "./actionTitle";
import {
  codexProfileInstanceId,
  codexProfileTabId,
  inspectCodexProfiles,
  manageCodexProfiles,
  releaseCodexProfile,
  sweepCodexProfiles,
  type ProfileSweepHost,
} from "./profile-sweep";
import {
  classifyCodexTabState,
  type TabStateClassification,
  type TabStateSnapshot,
} from "./tabState";
import {
  detectSessionId as detectCodexSessionId,
  detectSessionModel as detectCodexSessionModel,
  detectSessionReasoningEffort as detectCodexSessionReasoningEffort,
  enumerateSessions as enumerateCodexSessions,
  extractSessionUuid,
  findSession as findCodexSession,
  sessionFilePath as resolveCodexSessionFilePath,
  sessionExists as codexSessionExists,
} from "./sessions";
import {
  checkIntegration as checkCodexIntegration,
  ensureIntegrationOnStartup as ensureCodexIntegrationOnStartup,
  grantIntegrationTrust as grantCodexIntegrationTrust,
  installIntegration as installCodexIntegration,
  restoreIntegration as restoreCodexIntegration,
  uninstallIntegration as uninstallCodexIntegration,
  type CodexIntegrationInstallOptions,
} from "./integration";
export {
  codexProfileInstanceId,
  codexProfileTabId,
  inspectCodexProfiles,
  manageCodexProfiles,
  releaseCodexProfile,
  sweepCodexProfiles,
};

function profileSweepHost(): ProfileSweepHost {
  return {
    platform: () => host.platform(),
    homeDir: () => host.homeDir(),
    log: (level, message) => host.log(level, message),
    fs: {
      fileExists: (path) => host.fs.fileExists(path),
      isSymlink: (path) => host.fs.isSymlink(path),
      readDir: (path) => host.fs.readDir(path),
      readFile: (path) => host.fs.readFile(path),
      removeFile: (path) => host.fs.removeFile(path),
      writeFileAtomic: (path, contents) => host.fs.writeFileAtomic(path, contents),
      lockFile: (path) => host.fs.lockFile(path),
      unlockFile: (id) => host.fs.unlockFile(id),
    },
  };
}

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
  if (params.starterPromptText) return params.starterPromptText;
  if (params.starterPrompt === "no_starter") return undefined;
  if (params.starterPrompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (params.starterPrompt === "team_bootstrap") throw new Error("team_bootstrap requires host starterPromptText");
  return ASSIGNED_TASK_START_PROMPT;
}

function windowsProfileCommand(parts: string[], taskPath: string | null, instanceId: string): string {
  const quotedInstanceId = host.shell.quoteFor(instanceId, host.platform());
  const taskSetup = taskPath
    ? `$ctTaskInstructions = [System.IO.File]::ReadAllText(${host.shell.quoteFor(taskPath, host.platform())}, [System.Text.Encoding]::UTF8); `
      + "$ctTaskJson = ($ctTaskInstructions | ConvertTo-Json -Compress); "
    : "$ctTaskJson = $null; ";
  const profileValue = taskPath
    ? "('# codeterm-instance: ' + $ctInstanceId + [Environment]::NewLine + 'developer_instructions = ' + $ctTaskJson)"
    : "('# codeterm-instance: ' + $ctInstanceId)";
  const prefix = "$ctTabId = $env:CODETERM_TAB_ID; if (-not $ctTabId) { throw 'CODETERM_TAB_ID is required for a per-tab Codex profile' }; "
    + taskSetup
    + "$ctProfileSuffix = $ctTabId -replace '[^A-Za-z0-9_-]', '-'; "
    + "$ctProfileName = 'codeterm-' + $ctProfileSuffix; "
    + "$ctProfilePath = Join-Path $env:USERPROFILE ('.codex\\' + $ctProfileName + '.config.toml'); "
    + `$ctInstanceId = ${quotedInstanceId}; `
    + "if (Test-Path -LiteralPath $ctProfilePath) { $ctPriorReleased = $false; try { if ($env:CODETERM_API_URL -and $env:CODETERM_API_TOKEN) { "
    + "$ctPriorReleaseUri = $env:CODETERM_API_URL.TrimEnd('/') + '/api/tabs/' + [uri]::EscapeDataString($ctTabId) + '/launch-artifacts/release'; "
    + "$ctPriorRelease = Invoke-RestMethod -Method Post -Uri $ctPriorReleaseUri -Headers @{ Authorization = ('Bearer ' + $env:CODETERM_API_TOKEN) } -ContentType 'application/json' -Body '{}' -ErrorAction Stop; "
    + "$ctPriorReleased = [bool]$ctPriorRelease.released } } catch { Write-Warning 'Codex project trust carry-over was not confirmed'; }; "
    + "if (-not $ctPriorReleased) { $ctOwnsPriorProfile = $false; try { $ctOwnsPriorProfile = ([System.IO.File]::ReadAllLines($ctProfilePath) | Select-Object -First 1) -eq ('# codeterm-instance: ' + $ctInstanceId) } catch {}; "
    + "if ($ctOwnsPriorProfile) { $ctPreservedProfilePath = Join-Path $env:USERPROFILE ('.codex\\' + $ctProfileName + '-preserved.config.toml'); "
    + "$ctExistingPreserved = Get-ChildItem -LiteralPath (Join-Path $env:USERPROFILE '.codex') -Filter ($ctProfileName + '-preserved*.config.toml') -File -ErrorAction SilentlyContinue | Select-Object -First 1; "
    + "if (-not $ctExistingPreserved -and -not (Test-Path -LiteralPath $ctPreservedProfilePath)) { try { [System.IO.File]::Move($ctProfilePath, $ctPreservedProfilePath); Write-Warning 'Unreleased Codex profile was preserved'; } catch {} } }; "
    + "if ((Test-Path -LiteralPath $ctProfilePath) -or -not $ctOwnsPriorProfile) { $ctProfileName = $ctProfileName + '-retry-' + $PID + '-' + [DateTime]::UtcNow.Ticks; $ctProfilePath = Join-Path $env:USERPROFILE ('.codex\\' + $ctProfileName + '.config.toml'); "
    + "Write-Warning 'Unreleased Codex profile was retained; using a separate profile for this launch'; } } }; "
    + `[System.IO.File]::WriteAllText($ctProfilePath, ${profileValue}, [System.Text.UTF8Encoding]::new($false)); `
    + "$ctProfileReleased = $false; try { try { ";
  const suffix = " } finally { if (Test-Path -LiteralPath $ctProfilePath) { $ctProfileReleased = $false; "
    + "try { if ($env:CODETERM_API_URL -and $env:CODETERM_API_TOKEN) { "
    + "$ctReleaseUri = $env:CODETERM_API_URL.TrimEnd('/') + '/api/tabs/' + [uri]::EscapeDataString($ctTabId) + '/launch-artifacts/release'; "
    + "$ctRelease = Invoke-RestMethod -Method Post -Uri $ctReleaseUri -Headers @{ Authorization = ('Bearer ' + $env:CODETERM_API_TOKEN) } -ContentType 'application/json' -Body '{}' -ErrorAction Stop; "
    + "$ctProfileReleased = [bool]$ctRelease.released } } catch { Write-Warning 'Codex profile retained because project trust could not be persisted'; }; "
    + "} } } finally { if ($ctProfileReleased -and (Test-Path -LiteralPath $ctProfilePath)) { Remove-Item -LiteralPath $ctProfilePath -Force -ErrorAction SilentlyContinue } }";
  return prefix + parts.join(" ") + suffix;
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
  starterPromptText(prompt: "no_starter" | "assigned_task" | "idle_wakeup" | "team_bootstrap" | "startup_handshake"): string | undefined {
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

  parseDirectoryTrustPrompt(text: string): ParsedPrompt | null {
    return parseCodexFolderTrustPrompt(String(text || ""));
  },

  buildPromptResponse(optionIndex: number, screen?: string): string[] {
    return responseKeysForIndex(optionIndex, String(screen || ""));
  },

  directoryTrustPromptResponse(screen: string): string[] | null {
    const view = String(screen || "");
    const prompt = classifyCodexFolderTrustPrompt(view);
    if (!prompt || prompt.kind !== "active") return null;
    if (prompt.affirmativeIndex !== 0) return [];
    if (!prompt.affirmativeSelected && responseKeysForIndex(prompt.affirmativeIndex, view)[0] === "DQ==") {
      return [];
    }
    return responseKeysForIndex(prompt.affirmativeIndex, view).slice(0, 1);
  },

  // Codex CLI has no model-list subcommand; its local cache is the only sync account-scoped source. Missing => seed.
  discoverModels(): ModelInfo[] | null {
    return discoverCodexModels();
  },

  releaseTabLaunchArtifacts(tabId: string, instanceId: string): boolean {
    return releaseCodexProfile(profileSweepHost(), tabId, instanceId);
  },

  sweepLaunchArtifacts(request: {
    instanceId: string;
    liveTabIds: string[];
    confirm: boolean;
    includeLegacy: boolean;
  }) {
    return manageCodexProfiles(
      profileSweepHost(),
      request.liveTabIds,
      request.instanceId,
      request.confirm,
      request.includeLegacy,
    );
  },

  buildLaunchCommand(params: LaunchParams): string {
    const p = params || {};
    const parts: string[] = ["codex", "-c", "check_for_update_on_startup=false"];
    if (p.skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
      parts.push("--dangerously-bypass-hook-trust");
    }
    if (p.args && p.args.length > 0) {
      // Quote each caller-supplied arg to prevent shell injection.
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    if (p.task?.trim()) {
      const instanceId = typeof p.instanceId === "string" && /^[A-Za-z0-9_-]+$/.test(p.instanceId)
        ? p.instanceId
        : null;
      if (host.platform() === "windows" && !instanceId) {
        return "Write-Error 'CodeTerm refused the Codex launch: instance identity is missing, so a per-tab profile cannot be safely owned'; exit 1";
      }
      const instructions = withLaunchMarker(p.task, typeof p.launchMarker === "string" ? p.launchMarker : undefined);
      const taskPath = host.fs.writeTempFile(instructions, ".ct-task.md");
      if (!taskPath) throw new Error("failed to write Codex task instructions");
      const quotedPath = host.shell.quoteFor(taskPath, host.platform());
      if (host.platform() === "windows") {
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
        return windowsProfileCommand(parts, taskPath, instanceId!);
      }
    }
    return parts.join(" ");
  },

  buildResumeCommand(sessionId: string, skipPermissions?: boolean): string {
    const parts = ["codex", "-c", "check_for_update_on_startup=false", "resume"];
    if (skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
      parts.push("--dangerously-bypass-hook-trust");
    }
    // session_id accepts UUID or rollout filename stem.
    const uuid = extractSessionUuid(sessionId) || sessionId;
    parts.push(host.shell.quoteFor(uuid, host.platform()), "--no-daemon");
    return parts.join(" ");
  },

  buildResumeCommandWithContext(params: ResumeParams): string {
    const p = params || ({} as ResumeParams);
    const platform = host.platform();
    const parts = ["codex", "-c", "check_for_update_on_startup=false", "resume"];
    if (p.skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
      parts.push("--dangerously-bypass-hook-trust");
    }
    const uuid = extractSessionUuid(String(p.sessionId || "")) || String(p.sessionId || "");
    parts.push(host.shell.quoteFor(uuid, platform), "--no-daemon");
    if (Array.isArray(p.args)) {
      for (const arg of p.args) parts.push(host.shell.quoteFor(String(arg), platform));
    }
    if (platform === "windows") {
      const instanceId = typeof p.instanceId === "string" && /^[A-Za-z0-9_-]+$/.test(p.instanceId)
        ? p.instanceId
        : null;
      if (!instanceId) {
        return "Write-Error 'CodeTerm refused the Codex resume: instance identity is missing, so a per-tab profile cannot be safely owned'; exit 1";
      }
      parts.push("--profile", "$ctProfileName");
      return windowsProfileCommand(parts, null, instanceId);
    }
    return parts.join(" ");
  },

  sessionFilePath(_cwd: string, sessionId: string): string | null {
    return resolveCodexSessionFilePath(_cwd, sessionId);
  },

  sessionExists(cwd: string, sessionId: string): boolean {
    return codexSessionExists(cwd, sessionId);
  },

  detectLaunchSession(evidence: LaunchSessionEvidence): LaunchSessionDetection | null {
    return detectCodexLaunchSession(evidence);
  },

  // Codex 0.153 has no per-session JSONL transcript; keep the path capability honest.
  sessionJsonlPath(cwd: string, sessionId: string): string | null {
    return this.sessionFilePath!(cwd, sessionId);
  },

  // Duplicated by design across provider bundles (QuickJS, no imports) — parity guarded by provider-helper-parity.test.ts.
  parseSessionDelta(chunk: string, context?: SessionDeltaContext): unknown {
    const out: unknown[] = [];
    const commandPairing = commandPairingState(context);
    const activityIds = new Set<string>();
    // One source record can be reported twice (completion event and response item) under one id.
    const pushOnce = (row: { uuid?: string } | null) => {
      if (!row || (row.uuid && activityIds.has(row.uuid))) return;
      if (row.uuid) activityIds.add(row.uuid);
      out.push(row);
    };
    const lines = String(chunk || "").split("\n");
    let stamped = 0;
    const stampRows = (lineIndex: number) => {
      for (; stamped < out.length; stamped++) {
        (out[stamped] as { recordIndex?: number }).recordIndex = lineIndex;
      }
    };
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
      stampRows(i - 1);
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
      const normalizedEventType = v.payload?.type;
      if (normalizedEventType === "context_compacted") {
        out.push({
          role: "system",
          blocks: [{ kind: "text", data: { text: "context compacted" } }],
          ts,
          timestamp: tsIso,
          timelineMarker: { kind: "context_compaction" },
        });
        continue;
      }
      if (normalizedEventType === "thread_goal_updated") {
        const sessionIntent = sessionIntentFromPayload(v.payload!);
        if (!sessionIntent) continue;
        out.push({
          role: "custom",
          blocks: [],
          ts,
          timestamp: tsIso,
          sessionIntent,
        });
        continue;
      }
      if (v.type === "event_msg" && (v.payload?.type === "task_started" || v.payload?.type === "task_complete" || v.payload?.type === "turn_aborted")) {
        const turnId = typeof v.payload.turn_id === "string" ? v.payload.turn_id : "";
        if (!turnId) continue;
        clearTurnPairings(commandPairing, turnId);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_flow", data: { state: v.payload.type === "task_started" ? "start" : "end", flow_id: turnId } }],
          ts: ts,
          timestamp: tsIso,
        });
        continue;
      }
      if (v.type === "event_msg" && v.payload?.type === "item_completed") {
        if (pairCommandExecution(v.payload.item, v.payload.turn_id, commandPairing)) continue;
        pushOnce(completedActivity(v.payload.item, ts, tsIso, v.payload.turn_id));
        continue;
      }
      if (v.type === "event_msg" && v.payload?.type === "sub_agent_activity") {
        pushOnce(subAgentRow(v.payload as Record<string, unknown>, v.payload.event_id, undefined, ts, tsIso));
        continue;
      }
      if (v.type === "event_msg" && (v.payload?.type === "web_search_end" || v.payload?.type === "image_generation_end")) {
        const result = v.payload.type === "web_search_end"
          ? webSearchEndResult(v.payload, ts, tsIso)
          : imageGenerationEndResult(v.payload, ts, tsIso);
        if (result && !activityIds.has(result.uuid)) {
          activityIds.add(result.uuid);
          out.push(result);
        }
        continue;
      }
      if (v.type === "event_msg" && v.payload && v.payload.type === "patch_apply_end") {
        const p = v.payload;
        const content = typeof p.stdout === "string" ? p.stdout : "";
        const callId = typeof p.call_id === "string" ? p.call_id : "";
        const diffs = codexFileDiffs(p.changes);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_result", data: { content: content, tool_use_id: callId, ...(diffs.length > 0 ? { file_diffs: diffs } : {}) } }],
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
        const pending = p.type === "custom_tool_call_output" ? commandPairing.pending.get(callId) : undefined;
        if (pending) commandPairing.pending.delete(callId);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_result", data: {
            content: codexToolOutputText(p.output), tool_use_id: callId,
            ...(pending ? commandPairingResult(pending) : {}),
          } }],
          ts: ts,
          timestamp: tsIso,
        });
      } else if (p.type === "reasoning") {
        pushOnce(reasoningResponseRow(p as Record<string, unknown>, ts, tsIso));
      } else if (p.type === "agent_message") {
        pushOnce(agentMessageRow(p as Record<string, unknown>, ts, tsIso));
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
        const message = {
          role: "assistant",
          blocks: [{ kind: "tool_use", data: { name: name, id: callId, input: input, ...canonical } }],
          ts: ts,
          timestamp: tsIso,
        };
        const commands = name === "exec" && callId && typeof input.raw === "string"
          ? execScriptCommands(input.raw)
          : [];
        out.push(message);
        if (commands.length > 0) {
          const turnId = callTurnId(p);
          if (!turnId) continue;
          commandPairing.pending.delete(callId);
          commandPairing.pending.set(callId, {
            commandCount: commands.length,
            commands,
            completedCommands: new Set(),
            commandResults: [],
            turnId,
          });
          trimPendingCalls(commandPairing);
        }
      }
    }
    if (commandPairing.sessionKey && commandPairing.pending.size === 0) {
      commandPairingSessions.delete(commandPairing.sessionKey);
    }
    stampRows(lines.length - 1);
    return { messages: out };
  },

  detectSessionId(cwd: string, exclude?: string[] | string, maxAgeMs?: number): string | null {
    return detectCodexSessionId(cwd, exclude, maxAgeMs);
  },

  checkIntegration(): unknown {
    return checkCodexIntegration();
  },

  installIntegration(
    apiPort: number,
    options?: { consent?: boolean },
  ): unknown {
    return installCodexIntegration(apiPort, options);
  },

  // Codex is the only hash authority; the grant lane never computes a trust hash CodeTerm-side.
  grantIntegrationTrust(managedKeys: string[]): unknown {
    return grantCodexIntegrationTrust(managedKeys);
  },

  uninstallIntegration(): unknown {
    return uninstallCodexIntegration();
  },

  restoreIntegration(): unknown {
    return restoreCodexIntegration();
  },

  ensureIntegrationOnStartup(apiPort: number): unknown {
    return ensureCodexIntegrationOnStartup(apiPort, (port, options) =>
      installCodexIntegration(port, options as CodexIntegrationInstallOptions));
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
    return enumerateCodexSessions();
  },

  findSession(sessionId: string): unknown | null {
    return findCodexSession(sessionId);
  },

  // Read the exact model identity from this session's latest turn context.
  detectSessionModel(cwd: string, sessionId: string): string | null {
    return detectCodexSessionModel(cwd, sessionId);
  },

  detectSessionReasoningEffort(cwd: string, sessionId: string): string | null {
    return detectCodexSessionReasoningEffort(cwd, sessionId);
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

  classifiedPromptAutoResponse(state: TabState, screen: string): "skip" | null {
    if (state !== "update_prompt") return null;
    const options = parseMenuOptions(screen);
    if (!options.some((option) => /\bupdate\b/i.test(option.label))) return null;
    return options.filter((option) => option.label === "Skip").length === 1 ? "skip" : null;
  },

};

// types shared by the parsers above

interface CodexContentBlock { type?: string; text?: string }
interface CodexPayload {
  item?: unknown;
  type?: string;
  role?: string;
  content?: string | CodexContentBlock[];
  goal?: unknown;
  thread_goal?: unknown;
  title?: unknown;
  thread_name?: unknown;
  arguments?: string;
  name?: string;
  output?: unknown;
  model?: string;
  input?: string | Record<string, unknown>;
  call_id?: string;
  stdout?: string;
  success?: boolean;
  turn_id?: unknown;
  id?: unknown;
  event_id?: unknown;
  summary?: unknown;
  author?: unknown;
  recipient?: unknown;
  internal_chat_message_metadata_passthrough?: unknown;
  changes?: unknown;
}
interface CodexEntry { type?: string; payload?: CodexPayload; timestamp?: string }

interface PendingCommandPairing {
  commandCount: number;
  commands: string[];
  completedCommands: Set<number>;
  commandResults: Record<string, unknown>[];
  turnId: string;
}

interface CommandPairingState {
  pending: Map<string, PendingCommandPairing>;
  sessionKey?: string;
  lastOffset?: number;
}

const commandPairingSessions = new Map<string, CommandPairingState>();
const MAX_PAIRING_SESSIONS = 32;
const MAX_PENDING_CALLS_PER_SESSION = 64;

function commandPairingState(context?: SessionDeltaContext): CommandPairingState {
  const sessionKey = typeof context?.session_key === "string" ? context.session_key : "";
  const offset = context?.from_offset;
  if (!sessionKey || typeof offset !== "number" || !Number.isSafeInteger(offset) || offset < 0) {
    return { pending: new Map() };
  }
  let state = commandPairingSessions.get(sessionKey);
  if (state && state.lastOffset !== undefined && offset <= state.lastOffset) {
    commandPairingSessions.delete(sessionKey);
    state = undefined;
  }
  if (!state) state = { pending: new Map(), sessionKey };
  state.lastOffset = offset;
  commandPairingSessions.delete(sessionKey);
  commandPairingSessions.set(sessionKey, state);
  while (commandPairingSessions.size > MAX_PAIRING_SESSIONS) {
    const oldest = commandPairingSessions.keys().next().value;
    if (typeof oldest !== "string") break;
    commandPairingSessions.delete(oldest);
  }
  return state;
}

function trimPendingCalls(state: CommandPairingState): void {
  while (state.pending.size > MAX_PENDING_CALLS_PER_SESSION) {
    const oldest = state.pending.keys().next().value;
    if (typeof oldest !== "string") break;
    state.pending.delete(oldest);
  }
}

function clearTurnPairings(state: CommandPairingState, turnId: string): void {
  for (const [callId, call] of state.pending) {
    if (call.turnId === turnId) state.pending.delete(callId);
  }
}

function commandPairingResult(call: PendingCommandPairing): Record<string, unknown> {
  if (call.commandResults.length === 0) return {};
  const results = [...call.commandResults].sort((left, right) => Number(left.command_index) - Number(right.command_index));
  const failed = results.find((result) => result.tool_status === "error");
  const complete = results.length === call.commandCount;
  const durations = results.map((result) => result.tool_duration_ms);
  const completeDurations = complete && durations.every((duration) => typeof duration === "number");
  const failureCode = failed && typeof failed.tool_exit_code === "number" && failed.tool_exit_code !== 0
    ? failed.tool_exit_code
    : undefined;
  return {
    tool_wrapper_detail: results.map((result) => result.wrapper_detail).join("\n\n"),
    tool_command_results: results,
    ...(failed ? { tool_status: "error", is_error: true } : complete ? { tool_status: "success", is_error: false } : {}),
    ...(failureCode !== undefined ? { tool_exit_code: failureCode } : complete ? { tool_exit_code: 0 } : {}),
    ...(completeDurations ? { tool_duration_ms: durations.reduce((sum, duration) => sum + Number(duration), 0) } : {}),
  };
}

function callTurnId(payload: CodexPayload): string | undefined {
  if (typeof payload.turn_id === "string") return payload.turn_id;
  const metadata = payload.internal_chat_message_metadata_passthrough;
  if (!metadata || typeof metadata !== "object" || Array.isArray(metadata)) return undefined;
  const turnId = (metadata as Record<string, unknown>).turn_id;
  return typeof turnId === "string" && turnId ? turnId : undefined;
}

function pairCommandExecution(item: unknown, turnId: unknown, state: CommandPairingState): boolean {
  if (!item || typeof item !== "object" || Array.isArray(item)) return false;
  const execution = item as Record<string, unknown>;
  if (execution.type !== "CommandExecution") return false;
  const wrapper = unwrapCommandExecution(execution.command);
  const currentTurn = typeof turnId === "string" ? turnId : "";
  if (!wrapper || !currentTurn) return false;
  const candidates = Array.from(state.pending.values()).filter((call) =>
    call.turnId === currentTurn && call.commands.some((command, index) =>
      command === wrapper.command && !call.completedCommands.has(index)));
  if (candidates.length !== 1) return false;
  const call = candidates[0];
  const commandIndex = call.commands.findIndex((command, index) =>
    command === wrapper.command && !call.completedCommands.has(index));
  const activity = completedActivity(item, null, null, turnId);
  const toolData = activity?.blocks.find((block) => block.kind === "tool_use")?.data;
  if (!toolData) return false;
  call.completedCommands.add(commandIndex);
  call.commandResults.push({
    command_index: commandIndex,
    command: wrapper.command,
    wrapper_detail: wrapper.detail,
    ...(toolData.tool_status !== undefined ? { tool_status: toolData.tool_status } : {}),
    ...(toolData.tool_exit_code !== undefined ? { tool_exit_code: toolData.tool_exit_code } : {}),
    ...(toolData.tool_duration_ms !== undefined ? { tool_duration_ms: toolData.tool_duration_ms } : {}),
  });
  return true;
}

export interface CodexHookCmd { type?: string; command?: string; commandWindows?: string; timeout?: number; _source?: string }

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
export interface CodexHookEntry { matcher?: string; hooks?: CodexHookCmd[] }

function canonicalToolFields(name: string, input: Record<string, unknown>): Record<string, unknown> {
  const fields = baseToolFields(name, input);
  return { ...fields, tool_title: actionTitle(fields) };
}

function baseToolFields(name: string, input: Record<string, unknown>): Record<string, unknown> {
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

function isValidCodexSessionDelta(v: CodexEntry): boolean {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type === "event_msg") {
    if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
    return v.payload.type === "patch_apply_end"
      || v.payload.type === "item_completed"
      || v.payload.type === "web_search_end"
      || v.payload.type === "image_generation_end"
      || v.payload.type === "task_started"
      || v.payload.type === "task_complete"
      || v.payload.type === "turn_aborted"
      || v.payload.type === "context_compacted"
      || (v.payload.type === "thread_goal_updated" && sessionIntentFromPayload(v.payload) !== null)
      || v.payload.type === "sub_agent_activity";
  }
  if (v.type !== "response_item") return false;
  if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
  const kind = v.payload.type;
  if (
    kind !== "message" &&
    kind !== "reasoning" &&
    kind !== "agent_message" &&
    kind !== "function_call" &&
    kind !== "function_call_output" &&
    kind !== "custom_tool_call" &&
    kind !== "custom_tool_call_output" &&
    kind !== "context_compacted" &&
    kind !== "thread_goal_updated"
  ) {
    return false;
  }
  return true;
}

function cleanIntentValue(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const cleaned = value.trim();
  return cleaned && !/[\u0000-\u001f\u007f]/.test(cleaned) ? cleaned : undefined;
}

function sessionIntentFromPayload(payload: CodexPayload): Record<string, string> | null {
  const title = cleanIntentValue(payload.title) || cleanIntentValue(payload.thread_name);
  const goal = cleanIntentValue(payload.goal) || cleanIntentValue(payload.thread_goal);
  if (!title && !goal) return null;
  return {
    ...(title ? { title } : {}),
    ...(goal ? { goal } : {}),
  };
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
    || name === "list_agents";
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
