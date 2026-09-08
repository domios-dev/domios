import type { LaunchParams, ModelInfo, ParsedPrompt, PluginModule } from "@codeterm/plugin-sdk";
import { discoverCodexModels } from "./models";
import {
  parseCodexPrompt,
  responseKeysForIndex,
} from "./prompt";
import { fetchCodexUsage, parseCodexUsage } from "./usage";
import { summarizeExecScript, unwrapShellCommand } from "./execWrapper";
import { patchEdits } from "./patchEdits";
import {
  classifyCodexTabState,
  type TabStateClassification,
  type TabStateSnapshot,
} from "./tabState";
import {
  detectSessionId as detectCodexSessionId,
  detectSessionModel as detectCodexSessionModel,
  enumerateSessions as enumerateCodexSessions,
  extractSessionUuid,
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
// Not yet called from ensureIntegrationOnStartup — apiPort alone carries no safe live-tab-id source.
export { sweepCodexProfiles, codexProfileTabId, codexProfilesToSweep, type SweepHost } from "./profile-sweep";

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
      parts.push("--dangerously-bypass-hook-trust");
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
        prefix = `$ctTaskInstructions = [System.IO.File]::ReadAllText(${quotedPath}, [System.Text.Encoding]::UTF8); `
          + "$ctTaskJson = ($ctTaskInstructions | ConvertTo-Json -Compress); "
          + "$ctProfileSuffix = if ($env:CODETERM_TAB_ID) { $env:CODETERM_TAB_ID -replace '[^A-Za-z0-9_-]', '-' } else { [guid]::NewGuid().ToString('N') }; "
          + "$ctProfileName = 'codeterm-' + $ctProfileSuffix; "
          + "$ctProfilePath = Join-Path $env:USERPROFILE ('.codex\\' + $ctProfileName + '.config.toml'); "
          + "Set-Content -LiteralPath $ctProfilePath -Encoding UTF8 -Value ('developer_instructions = ' + $ctTaskJson); "
          + "try { ";
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
      parts.push("--dangerously-bypass-hook-trust");
    }
    // session_id accepts UUID or rollout filename stem.
    const uuid = extractSessionUuid(sessionId) || sessionId;
    parts.push(host.shell.quoteFor(uuid, host.platform()));
    return parts.join(" ");
  },

  sessionFilePath(_cwd: string, sessionId: string): string | null {
    return resolveCodexSessionFilePath(_cwd, sessionId);
  },

  sessionExists(cwd: string, sessionId: string): boolean {
    return codexSessionExists(cwd, sessionId);
  },

  // Codex 0.153 has no per-session JSONL transcript; keep the path capability honest.
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

  // Read the exact model identity from this session's latest turn context.
  detectSessionModel(cwd: string, sessionId: string): string | null {
    return detectCodexSessionModel(cwd, sessionId);
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
