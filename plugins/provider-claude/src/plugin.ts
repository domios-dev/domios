import { writeClaudeLaunchPrompt } from "./launchPrompt";
import { isImageDisplayMetadata } from "./imageMetadata";
import { checkClaudeUpdate } from "./update";
import { claudeTrustRecord } from "./preTrust";
import { updatePromptSafeChoice } from "../../provider-shared/updatePrompt";
// plugin.js by scripts/build-plugin.mjs. Frontend contract lives in src/index.ts.
import type {
  LaunchParams,
  LaunchSessionDetection,
  LaunchSessionEvidence,
  ParsedPrompt,
  PluginModule,
  ResumeParams,
} from "@codeterm/plugin-sdk";
import { manifest } from "./shared";
import { permissionHookCapability } from "./permissionHook";
import { parseContextUsage, statuslineLaunchArgs } from "./context";
import { authStatusProbe, parseAuthStatus, parseSessionObservations } from "./signals";
import {parseBackgroundWork} from "./backgroundWork";
import { detectClaudeLaunchSession } from "./launchSession";
import {
  fetchClaudeUsage,
  parseFetchedClaudeUsage,
  usageBlockIsComplete,
  usageNormalize,
  usageParseBlock,
} from "./usage";
import {
  ClaudeEntry,
  aiTitleFromEntry,
  ClaudeToolUseResult,
  claudeSessionFilePath,
  detectSessionModelAfterBoundary,
  editedTextFilePath,
  extractSetModelFamily,
  familyFromModelId,
  isValidClaudeSessionMessage,
  sessionModelTranscriptBoundary,
  validateEntry,
} from "./session";
import { discoverAnthropicModels, manifestModels } from "./models";
import {reasoningEffortLaunchArgsForModel} from "./modelCapabilities";
import { claudeFileDiffs } from "./fileDiffs";
import {
  isAgentActivityBlock,
  isSubAgentCall,
  subAgentCallBlock,
  subAgentLaunchBlock,
  subAgentNotificationBlock,
  subAgentUsageSources,
  xmlTagBody,
} from "./subagents";
import { canonicalToolFields, toolDescription } from "./toolFields";
import {
  classifyClaudeTabState,
  TabStateSnapshot,
  TabStateClassification,
} from "./classify";
import {
  buildClaudePromptResponse,
  claudeOnboardingPrompt,
  claudeOnboardingNotNowStep,
  parseClaudeLetteredPrompt,
  parseClaudeNumberedPrompt,
  parseClaudeWizardPrompt,
} from "./prompts";
import { claudePermissionPromptResponse } from "./permissionPrompt";
import { claudeFolderTrustPrompt, claudeFolderTrustResponse } from "./folderTrust";
import {
  CODETERM_HOOK_MARKER,
  CODETERM_HOOK_MARKER_ARG,
  INTEGRATION_HOOK_KEYS,
  entryCarriesMarker,
  entryIsLegacyPaneCodetermHook,
  entriesCarryMarker,
  isCurrentHookCommand,
  isCodetermHook,
} from "./hookOwnership";
import type { ClaudeSettings, HookEntry } from "./hookOwnership";

const TITLE_RE = /claude/i;
// v2.1+ TUIs prefix the title with a braille spinner glyph or ✳/✻.
const BRAILLE_LO = 0x2801;
const BRAILLE_HI = 0x28ff;
const QUARTER_CIRCLE_LO = 0x25d0;
const QUARTER_CIRCLE_HI = 0x25d3;

const OUTPUT_FINGERPRINTS = [
  "Claude Code",
  "Do you want to proceed?",
  "may I proceed",
  "Would you like me to",
  "Yes, and don't ask again",
  "Enter to confirm",
  "Cogitated for",
  "Brewed for",
  "Reasoned for",
];

const NOISE_FRAGMENTS = [
  "bypass permissions on",
  "shift+tab to cycle",
  "Update available! Run:",
  "brew upgrade claude-code",
  "esc to interrupt",
  "? for shortcuts",
  "ctrl+g to edit",
  "Monthly spend limit",
  "hold Space",
];

const FILE_TASK_PROMPT =
  "Execute the CodeTerm task provided in the appended system prompt. Follow its reporting instructions exactly.";
const IDLE_WAKEUP_START_PROMPT = "You are a newly started agent. Wait for instructions from the user or your orchestration parent.";

function starterPrompt(prompt: LaunchParams["starterPrompt"], hostText?: string): string | undefined {
  if (hostText) return hostText;
  if (prompt === "no_starter") return undefined;
  if (prompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (prompt === "team_bootstrap") throw new Error("team_bootstrap requires host starterPromptText");
  return FILE_TASK_PROMPT;
}

function taskNotificationStatus(content: string): string | null {
  const trimmed = content.trim();
  const start = trimmed.indexOf("<task-notification>");
  const end = trimmed.indexOf("</task-notification>", start);
  if (start < 0 || end < 0) {
    return null;
  }
  const block = trimmed.slice(start, end + "</task-notification>".length);
  const status = xmlTagBody(block, "status");
  return status ? `Task ${status}` : "Task notification";
}

function isCompactSummaryRow(entry: ClaudeEntry, content: string): boolean {
  if (entry.isCompactSummary === true) return true;
  return entry.isVisibleInTranscriptOnly === true
    && content.trimStart().startsWith("This session is being continued from a previous conversation");
}

const TUI_FRAGMENTS = [
  "? for shortcuts",
  "for shortcuts",
  "ctrl+l",
  "esc to interrupt",
  "bypass permissions",
  "shift+tab to cycle",
];

const SESSION_EXPLICIT_RE = /session(?:\s+id)?\s*[:=]\s*([a-z]+-[a-z]+-\d{2,6})\b/i;
const SESSION_BARE_RE = /(?:^|\n)\s*([a-z]{4,10}-[a-z]{4,10}-\d{4})\s*(?:$|\n)/m;

function sessionProjectPath(head: string): string | null {
  const lines = String(head || "").split("\n", 64);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]?.trim();
    if (!line) continue;
    try {
      const entry = JSON.parse(line) as { cwd?: unknown };
      if (typeof entry.cwd === "string" && entry.cwd.trim()) return entry.cwd;
    } catch {}
  }
  return null;
}

function isBrailleSpinner(ch: string): boolean {
  if (!ch) return false;
  const code = ch.charCodeAt(0);
  return (
    (code >= BRAILLE_LO && code <= BRAILLE_HI) ||
    (code >= QUARTER_CIRCLE_LO && code <= QUARTER_CIRCLE_HI) ||
    ch === "✳" ||
    ch === "✻"
  );
}


function retainForeignPrecompactEntry(entry: HookEntry): boolean {
  if (!entry || !Array.isArray(entry.hooks)) return true;
  const previous = entry.hooks;
  const remaining = previous.filter((hook) => !isCodetermHook({ hooks: [hook] }));
  if (remaining.length === previous.length) return true;
  entry.hooks = remaining;
  return remaining.length > 0;
}

const plugin: PluginModule = {
  checkForUpdate: checkClaudeUpdate,
  authStatusProbe,
  tracksBackgroundWork: () => true,
  parseBackgroundWork,
  reasoningEffortLaunchArgsForModel,
  modelCatalogueSeed: manifestModels,
  parseAuthStatus,
  parseSessionObservations,
  permissionHookCapability,
  parseContextUsage,
  starterPromptText(prompt: "no_starter" | "assigned_task" | "idle_wakeup" | "team_bootstrap" | "startup_handshake"): string | undefined {
    return prompt === "team_bootstrap" ? undefined : starterPrompt(prompt);
  },
  detectFromTitle(title: string): boolean {
    const t = String(title || "");
    if (TITLE_RE.test(t)) return true;
    const first = t.charAt(0);
    return isBrailleSpinner(first) && t.length > 2;
  },

  detectFromOutput(text: string): boolean {
    const t = String(text || "");
    for (let i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
      if (t.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) return true;
    }
    return false;
  },

  screenHasTui(screen: string): boolean {
    const s = String(screen || "");
    const lower = s.toLowerCase();
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    // Banner logo quadrant blocks — never in plain shell output.
    return s.indexOf("▛") !== -1 || s.indexOf("▜") !== -1;
  },

  isOutputNoise(text: string): boolean {
    const trimmed = String(text || "").trim();
    if (!trimmed) return true;
    for (let i = 0; i < NOISE_FRAGMENTS.length; i++) {
      if (trimmed.indexOf(NOISE_FRAGMENTS[i]) !== -1) return true;
    }
    // All-box-drawing line is decorative chrome.
    for (let j = 0; j < trimmed.length; j++) {
      const c = trimmed[j];
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
    if (c.indexOf("❯") !== -1) return true;
    if (c.indexOf("Enter to select") !== -1) return true;
    if (c.indexOf("Submit") !== -1 && (c.indexOf("☐") !== -1 || c.indexOf("✔") !== -1)) return true;
    if (c.indexOf("proceed") !== -1) return true;
    if (c.indexOf("Esc to cancel") !== -1) return true;
    if (c.indexOf("Tab to amend") !== -1) return true;
    if (c.indexOf("1.") !== -1 && c.indexOf("2.") !== -1) return true;
    for (let i = 0; i < 10; i++) {
      if (c.indexOf(" for " + i) !== -1) return true;
    }
    return false;
  },

  extractSessionName(screen: string): string | null {
    const s = String(screen || "");
    let m = SESSION_EXPLICIT_RE.exec(s);
    if (m && m[1]) return m[1];
    m = SESSION_BARE_RE.exec(s);
    if (m && m[1]) return m[1];
    return null;
  },

  describeSession(_filePath: string, head?: string) {
    const projectPath = sessionProjectPath(head ?? "");
    return projectPath ? { projectPath } : null;
  },

  detectEvents(text: string): string[] {
    const t = String(text || "");
    const events: string[] = [];
    if (parseClaudeWizardPrompt(t) || parseClaudeNumberedPrompt(t)) {
      events.push("permission_request");
    }
    // TaskComplete: "<CapitalizedVerb> for <digits><s|m|h>"
    const lines = t.split("\n");
    const start = Math.max(0, lines.length - 10);
    for (let i = start; i < lines.length; i++) {
      const line = lines[i].trim();
      const pos = line.indexOf(" for ");
      if (pos < 1) continue;
      const word = line.substring(0, pos);
      const after = line.substring(pos + 5);
      if (!/^[A-Z][a-zA-Z]*$/.test(word)) continue;
      if (/^\d+[smh]/.test(after) || (/^\d/.test(after) && /[smh]/.test(after))) {
        events.push("task_complete");
        break;
      }
    }
    return events;
  },

  updatePromptSafeChoice,

  parsePrompt(text: string): ParsedPrompt | null {
    const onboarding = claudeOnboardingPrompt(text);
    return (onboarding && { question: onboarding.question, options: onboarding.options })
      || claudeFolderTrustPrompt(text)
      || parseClaudeWizardPrompt(text)
      || parseClaudeNumberedPrompt(text)
      || parseClaudeLetteredPrompt(text);
  },

  parseDirectoryTrustPrompt(text: string): ParsedPrompt | null {
    const prompt = claudeFolderTrustPrompt(String(text || ""));
    return prompt && {
      question: prompt.question,
      options: prompt.options.map(([label], index): [string, string] => [label, String(index + 1)]),
    };
  },

  buildPromptResponse(optionIndex: number, screen?: string): string[] {
    return buildClaudePromptResponse(optionIndex, String(screen || ""));
  },

  preTrustDirectory(path: string) {
    return claudeTrustRecord(host, path, true);
  },

  isDirectoryTrusted(path: string): boolean {
    return claudeTrustRecord(host, path, false).status === "already_trusted";
  },

  directoryTrustPromptResponse(screen: string): string[] | null {
    return claudeFolderTrustResponse(screen);
  },

  launchOnboardingResponse(screen: string): { frameKey: string; step: string } | null {
    return claudeOnboardingNotNowStep(screen);
  },

  launchOnboardingSafeChoice(screen: string): string | null {
    return claudeOnboardingPrompt(screen)?.safeChoice ?? null;
  },

  permissionPromptResponse(toolName: string, prompt: ParsedPrompt, screen?: string) {
    return claudePermissionPromptResponse(toolName, prompt, screen);
  },

  // Launch command parity for native behavior across OS-specific execution modes.
  buildLaunchCommand(params: LaunchParams): string {
    const p = params || {};
    const win = host.platform() === "windows";
    const unset = win
      ? "Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; "
        + "$OutputEncoding = [Console]::OutputEncoding = [Console]::InputEncoding = "
        + "[System.Text.Encoding]::UTF8; "
        + "$PSNativeCommandArgumentPassing = 'Standard';"
      : "unset CLAUDECODE &&";
    const parts = [unset];

    parts.push("claude");
    if (p.skipPermissions) parts.push("--dangerously-skip-permissions");

    if (p.agentName) {
      parts.push("--agent");
      parts.push(host.shell.quoteFor(p.agentName, host.platform()));
    }
    if (p.sessionId) {
      parts.push("--session-id");
      parts.push(host.shell.quoteFor(p.sessionId, host.platform()));
    }
    const launchArgs = statuslineLaunchArgs(p.args || []);
    if (!launchArgs) {
      host.log("error", "[claude-launch] cannot compose launch settings");
      return "";
    }
    if (launchArgs.length > 0) {
      // Quote each caller-supplied arg — it reaches a shell (a mesh peer could inject `; cmd`).
      for (const arg of launchArgs) parts.push(quoteArg(String(arg)));
    }
    // Keep large worker prompts out of shell parsing and argv on every platform.
    {
      const visiblePath = writeClaudeLaunchPrompt(p.task || "");
      if (!visiblePath) return "";
      parts.push("--append-system-prompt-file");
      parts.push(host.shell.quoteFor(String(visiblePath), host.platform()));
      const prompt = p.task ? starterPrompt(p.starterPrompt, p.starterPromptText) : undefined;
      if (prompt) {
        parts.push("--");
        const framed = p.starterPromptPrefix ? p.starterPromptPrefix + prompt + "</domios>" : prompt;
        parts.push(host.shell.quoteFor(framed, host.platform()));
      }
    }
    return parts.join(" ");
  },

  buildResumeCommand(sessionId: string, skipPermissions?: boolean): string | null {
    const args = statuslineLaunchArgs([]);
    return args ? [resumeBase(sessionId, skipPermissions), ...args.map(quoteArg)].join(" ") : null;
  },

  // `--resume` restores the transcript but not the launch-time system prompt,
  // so it is re-applied through the same declared flag buildLaunchCommand uses.
  buildResumeCommandWithContext(params: ResumeParams): string | null {
    const p = params || ({} as ResumeParams);
    const resumeArgs = statuslineLaunchArgs(p.args || []);
    if (!resumeArgs) return null;


    const parts = [resumeBase(p.sessionId, p.skipPermissions)];
    if (resumeArgs.length > 0) {
      for (const arg of resumeArgs) parts.push(quoteArg(String(arg)));
    }
    const systemPrompt = p.systemPrompt;
    {
      const visiblePath = writeClaudeLaunchPrompt(systemPrompt || "");
      if (!visiblePath) return null;
      parts.push("--append-system-prompt-file");
      parts.push(host.shell.quoteFor(String(visiblePath), host.platform()));
    }
    return parts.join(" ");
  },

  // Tail session JSONL to find latest model (`message.model` or `Set model to ...`).
  detectSessionModel(cwd: string, sessionId: string): string | null {
    const path = claudeSessionFilePath(cwd, sessionId);
    if (!path || !host.fs.fileExists(path)) return null;
    const contents = host.fs.readFileTail(path, 4 * 1024 * 1024);
    if (!contents) return null;
    const lines = contents.split("\n");
    const declaration = manifest()?.commands?.modelSwitchCommands?.transcript;
    const declarationJson = declaration ? JSON.stringify(declaration) : null;
    for (let i = lines.length - 1; i >= 0; i--) {
      const line = lines[i];
      if (!line) continue;
      let v: ClaudeEntry;
      try { v = JSON.parse(line); } catch (_) { continue; }
      if (!v || !v.message) continue;
      const stdout = declarationJson ? host.commandTranscriptStdout(line, declarationJson) : null;
      const fam = stdout ? extractSetModelFamily(stdout) : null;
      if (fam) return fam;
      const m = v.message.model;
      if (typeof m === "string" && m && m !== "<synthetic>") {
        return familyFromModelId(m);
      }
    }
    return null;
  },

  sessionModelTranscriptBoundary,
  detectSessionModelAfterBoundary,

  detectSessionReasoningEffort(cwd: string, sessionId: string): string | null {
    const path = claudeSessionFilePath(cwd, sessionId);
    if (!path || !host.fs.fileExists(path)) return null;
    const contents = host.fs.readFileTail(path, 4 * 1024 * 1024);
    if (!contents) return null;
    const supported: Record<string, boolean> = { low: true, medium: true, high: true, xhigh: true, max: true };
    const commands = manifest()?.commands?.modelSwitchCommands;
    const declarationJson = commands?.transcript ? JSON.stringify(commands.transcript) : null;
    const confirmation = commands?.effort?.confirmation;
    const confirmations = confirmation ? Object.keys(supported).map(effort => ({
      effort, pattern: new RegExp(confirmation.replace("{effort}", effort).replace("(?i)", ""), "i"),
    })) : [];
    const lines = contents.split("\n");
    for (let i = lines.length - 1; i >= 0; i--) {
      if (!lines[i]) continue;
      let value: Record<string, unknown>;
      try { value = JSON.parse(lines[i]) as Record<string, unknown>; } catch (_) { continue; }
      const stdout = declarationJson ? host.commandTranscriptStdout(lines[i], declarationJson) : null;
      const confirmed = stdout ? confirmations.find(confirmation => confirmation.pattern.test(stdout)) : null;
      if (confirmed) return confirmed.effort;
      const message = value.message && typeof value.message === "object"
        ? value.message as Record<string, unknown>
        : null;
      const metadata = message?.metadata && typeof message.metadata === "object"
        ? message.metadata as Record<string, unknown>
        : null;
      const candidates = [value.effort, value.reasoning_effort, value.reasoningEffort, message?.effort, message?.reasoning_effort, metadata?.effort];
      for (let j = 0; j < candidates.length; j++) {
        const candidate = candidates[j];
        const effort = typeof candidate === "string" ? candidate.trim() : "";
        if (supported[effort]) return effort;
      }
    }
    return null;
  },

  // Discovery is genuine-only: null (no keys / failed fetch) keeps the host on
  // the registered manifest seed and must never be persisted as a live snapshot.
  discoverModels() {
    return discoverAnthropicModels();
  },

  // Parse the `/usage` TUI screen into a UsageSnapshot; null when nothing recognizable was found.
  parseUsage(rawText: string, nowMs: number): unknown {
    const raw = String(rawText || "");
    if (!raw.trim()) return null;
    const fetched = parseFetchedClaudeUsage(raw, nowMs);
    if (fetched !== undefined) return fetched;
    const text = usageNormalize(raw);

    const session = usageParseBlock(text, "Current session", false, nowMs);
    const weekly = usageParseBlock(text, "Current week\\s*\\(\\s*all\\s*models\\s*\\)", true, nowMs);

    if (session.pct === null && weekly.pct === null) return null;
    if (!usageBlockIsComplete(session) || !usageBlockIsComplete(weekly)) return null;
    return {
      provider: "claude",
      account_id: null,
      captured_at_ms: nowMs,
      session_pct: session.pct,
      session_resets_at_ms: session.resetsMs,
      weekly_pct: weekly.pct,
      weekly_resets_at_ms: weekly.resetsMs,
      raw_text: raw,
    };
  },

  fetchUsage(): unknown {
    return fetchClaudeUsage(host);
  },

  // Duplicated by design across provider bundles (QuickJS, no imports) — parity guarded by provider-helper-parity.test.ts. Change both or the test fails.
  // Parse `user`/`assistant` lines and map content blocks to SessionMessage.
  parseSessionMessages(text: string): unknown[] {
    const out: unknown[] = [];
    const lines = String(text || "").split("\n");
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      if (!line) continue;
      let v: ClaudeEntry;
      try { v = JSON.parse(line); } catch (e) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!isValidClaudeSessionMessage(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      const msgType = (v && v.type) || "";
      if (msgType !== "user" && msgType !== "assistant") continue;
      const isMeta = !!(v && v.isMeta);
      const msg = v.message;
      if (!msg) continue;
      const content = msg.content;
      let text2 = "";
      if (typeof content === "string") {
        text2 = content;
      } else if (Array.isArray(content)) {
        const chunks: string[] = [];
        for (let j = 0; j < content.length; j++) {
          const b = content[j];
          if (b && b.type === "text" && typeof b.text === "string") {
            chunks.push(b.text);
          }
        }
        text2 = chunks.join(" ");
      } else {
        continue;
      }
      if (!text2 || !text2.trim()) continue;
      let ts: number | null = null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1000);
      }
      out.push({ role: msgType, text: text2, ts: ts, isMeta: isMeta });
    }
    return out;
  },

  // Rust provides complete lines; plugin parses JSON into RichMessage[]. Byte offsets stay in Rust.
  parseSessionDelta(chunk: string): unknown {
    const out: unknown[] = [];
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
      const line = lines[i];
      if (!line) continue;
      let v: ClaudeEntry;
      try { v = JSON.parse(line); } catch (_) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      const emitObservations = () => {
        const observations = parseSessionObservations(v), background = parseBackgroundWork(v);
        if (!observations && !background.length) return false;
        out.push({role:"system",blocks:[],ts:null,uuid:null,
          provider_observations:observations, background_work:background,
          provider_session_id:typeof v.sessionId === "string" ? v.sessionId : null});
        return true;
      };
      if (!validateEntry(v)) {
        if (emitObservations()) continue;
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (isImageDisplayMetadata(v)) continue;
      const t = v && v.type;
      let ts: number | null = null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1000);
      }
      const timestamp = (typeof v.timestamp === "string") ? v.timestamp : null;

      if (t === "ai-title") {
        const title = aiTitleFromEntry(v);
        if (!title) continue;
        out.push({
          role: "custom",
          blocks: [],
          ts,
          uuid: (typeof v.uuid === "string") ? v.uuid : null,
          sessionIntent: { title },
          timestamp,
        });
        continue;
      }

      if (t === "edited_text_file" || (t === "attachment" && v.attachment?.type === "edited_text_file")) {
        const path = editedTextFilePath(v);
        if (!path) continue;
        out.push({
          role: "system",
          blocks: [],
          ts,
          uuid: (typeof v.uuid === "string") ? v.uuid : null,
          timelineMarker: { kind: "external_edit", path },
          timestamp,
        });
        continue;
      }

      const blocks: Array<{ kind: string; data: Record<string, unknown> }> = [];
      let role: string;
      let sourceSubtype: string | null = (typeof v.subtype === "string") ? v.subtype : null;
      let timelineMarker: Record<string, unknown> | undefined;
      if (t === "system") {
        // System rows carry top-level `content` + `subtype`; the host turns them into system/local_command cards.
        role = "system";
        const sysText = (typeof v.content === "string")
          ? v.content
          : (Array.isArray(v.content)
              ? v.content.filter((c) => c && c.type === "text")
                  .map((c) => c.text || "").join("\n")
              : "");
        const sdata: Record<string, unknown> = { text: sysText };
        if (typeof v.subtype === "string") sdata.subtype = v.subtype;
        if (typeof v.level === "string") sdata.level = v.level;
        blocks.push({ kind: "text", data: sdata });
      } else if (t === "user" || t === "assistant") {
        role = t;
        const msg = v.message;
        if (!msg) continue;
        const content = msg.content;
        if (typeof content === "string") {
          const agentNotice = t === "user" ? subAgentNotificationBlock(content, v) : null;
          const taskStatus = t === "user" && !agentNotice ? taskNotificationStatus(content) : null;
          if (agentNotice) {
            blocks.push(agentNotice);
          } else if (taskStatus) {
            role = "status";
            sourceSubtype = "task_notification";
            blocks.push({ kind: "text", data: { text: taskStatus, subtype: sourceSubtype } });
          } else if (t === "user" && isCompactSummaryRow(v, content)) {
            role = "system";
            sourceSubtype = "compact_summary";
            timelineMarker = { kind: "context_compaction" };
            blocks.push({ kind: "text", data: { text: content, subtype: sourceSubtype } });
          } else if (content) {
            blocks.push({ kind: "text", data: { text: content } });
          }
        } else if (Array.isArray(content)) {
          for (let j = 0; j < content.length; j++) {
            const b = content[j];
            if (!b || typeof b.type !== "string") continue;
            const data: Record<string, unknown> = {};
            for (const k in b) {
              if (Object.prototype.hasOwnProperty.call(b, k) && k !== "type") {
                data[k] = (b as Record<string, unknown>)[k];
              }
            }
            if (b.type === "tool_use" && isSubAgentCall(data.name, data.input)) {
              const agent = subAgentCallBlock(data.id, data.input);
              if (agent) {
                blocks.push(agent);
                continue;
              }
            }
            if (b.type === "tool_use") {
              const canonical = canonicalToolFields(
                typeof data.name === "string" ? data.name : "",
                data.input,
              );
              data.tool_kind = canonical.kind;
              data.tool_summary = canonical.summary;
              data.tool_detail = canonical.detail;
              data.tool_edits = canonical.edits;
              const description = toolDescription(data.input);
              if (description) data.tool_description = description;
              // Parallel calls from one API response share this id; it is the only proven link.
              if (typeof msg.id === "string" && msg.id) data.tool_flow_id = msg.id;
              const sourceId = exactSourceIdentity(v.uuid);
              if (sourceId) data.tool_call_source_id = sourceId;
            } else if (b.type === "thinking") {
              // The signature is opaque verification material, never display text.
              delete data.signature;
            }
            if (b.type === "tool_result") {
              const agent = subAgentLaunchBlock(data.tool_use_id, v.toolUseResult);
              if (agent) {
                blocks.push(agent);
                continue;
              }
              const outcome = commandOutcome(v.toolUseResult);
              if (outcome) Object.assign(data, outcome);
              const sourceId = exactSourceIdentity(v.sourceToolAssistantUUID);
              data.tool_pairing = sourceId ? "exact" : "unavailable";
              if (sourceId) data.tool_call_source_id = sourceId;
            }
            blocks.push({ kind: b.type, data: data });
          }
          const results = blocks.filter((block) => block.kind === "tool_result");
          // toolUseResult describes the row's single result; several results leave the link unproven.
          if (t === "user" && results.length === 1) {
            const diffs = claudeFileDiffs(v.toolUseResult);
            if (diffs.length > 0) results[0].data.file_diffs = diffs;
          }
        } else {
          continue;
        }
        // Sub-agent progress is not user input, whichever record carried it.
        if (blocks.length > 0 && blocks.every(isAgentActivityBlock)) role = "assistant";
      } else if (t === "attachment") {
        // Input sent while the agent was busy is queued as a queued_command attachment; surface it as the user message it is.
        const att = v.attachment;
        if (!att || att.type !== "queued_command" || typeof att.prompt !== "string" || !att.prompt) {
          emitObservations();
          continue;
        }
        const agentNotice = subAgentNotificationBlock(att.prompt, v);
        const taskStatus = agentNotice ? null : att.commandMode === "task-notification"
          ? taskNotificationStatus(att.prompt) ?? "Task notification"
          : taskNotificationStatus(att.prompt);
        if (agentNotice) {
          role = "assistant";
          blocks.push(agentNotice);
        } else if (taskStatus) {
          role = "status";
          sourceSubtype = "task_notification";
          blocks.push({ kind: "text", data: { text: taskStatus, subtype: sourceSubtype } });
        } else {
          role = "user";
          timelineMarker = { kind: "queued" };
          blocks.push({ kind: "text", data: { text: att.prompt } });
        }
      } else {
        emitObservations(); continue;
      }
      if (!blocks.length) {emitObservations(); continue;}
      if (t === "system" && sourceSubtype === "compact_boundary") {
        timelineMarker = { kind: "context_compaction" };
      }
      const context = parseContextUsage(v);
      out.push({
        ...(context ? { context_usage: context } : {}),
        provider_observations: parseSessionObservations(v),
        provider_session_id: typeof v.sessionId === "string" ? v.sessionId : null,
        background_work: parseBackgroundWork(v),
        role: role,
        blocks: blocks,
        ts: ts,
        uuid: (typeof v.uuid === "string") ? v.uuid : null,
        isMeta: v.isMeta === true ? true : null,
        sourceType: (typeof v.type === "string") ? v.type : null,
        sourceSubtype,
        level: (typeof v.level === "string") ? v.level : null,
        isCompactSummary: v.isCompactSummary === true ? true : null,
        isVisibleInTranscriptOnly: v.isVisibleInTranscriptOnly === true ? true : null,
        timelineMarker,
        timestamp,
      });
    }
    stampRows(lines.length - 1);
    return { messages: out };
  },

  // Keep JSONL path encoding aligned with `claudeSessionFilePath`.
  sessionJsonlPath(_cwd: string, sessionId: string): string | null {
    if (!sessionId) return null;
    return claudeSessionFilePath(_cwd, sessionId);
  },

  sessionUsageSources(sessionFilePath: string, sessionId: string): string[] {
    return subAgentUsageSources(sessionFilePath, sessionId);
  },

  // Write bridge rule to `~/.claude/rules/codeterm.md` via host APIs.
  installBridgeRule(payload: string): unknown {
    const m = manifest();
    const rulePath = (m && m.rule && m.rule.path) || "~/.claude/rules/codeterm.md";
    const expanded = host.expandHome(rulePath);
    if (!expanded) return { ok: false, error: "could not expand " + rulePath };
    const ok = host.writeFile(expanded, payload);
    if (ok) {
      host.log("info", "[claude-plugin] bridge rule installed to " + expanded);
      return { ok: true };
    }
    return { ok: false, error: "writeFile failed for " + expanded };
  },

  uninstallBridgeRule(): unknown {
    const m = manifest();
    const rulePath = (m && m.rule && m.rule.path) || "~/.claude/rules/codeterm.md";
    const expanded = host.expandHome(rulePath);
    if (!expanded) return { ok: true }; // nothing to do
    if (!host.fileExists(expanded)) return { ok: true };
    const ok = host.removeFile(expanded);
    if (ok) {
      host.log("info", "[claude-plugin] bridge rule removed from " + expanded);
      return { ok: true };
    }
    return { ok: false, error: "removeFile failed for " + expanded };
  },

  // Manage Claude settings hooks for Notification, Stop, SessionStart.
  // Hooks use a versioned source marker and runtime callback identity.

  checkIntegration(): unknown {
    const m = manifest();
    const path = host.expandHome((m && m.integration && m.integration.settingsPath)
      || "~/.claude/settings.json");
    if (!path || !host.fileExists(path)) {
      return {
        settings_exists: false,
        hooks_installed: false,
        integration_owned: false,
        notification_hook: false,
        permission_request_hook: false,
        stop_hook: false,

        session_start_hook: false,
        target: path || "~/.claude/settings.json",
        backup_exists: !!path && host.fileExists(path + ".codeterm-backup"),
        proposed_changes: ["PermissionRequest", "Notification", "Stop", "SessionStart"],
      };
    }
    const json = host.fs.readJson<ClaudeSettings>(path);
    if (!json) {
      return {
        settings_exists: true,
        hooks_installed: false,
        integration_owned: false,
        integration_state: "unreadable",
      };
    }
    function present(key: string, completion: boolean = false): boolean {
      const arr = json!.hooks && json!.hooks[key];
      if (!Array.isArray(arr)) return false;
      for (let i = 0; i < arr.length; i++) {
        const entry = arr[i];
        const hooks = entry && entry.hooks;
        if (!Array.isArray(hooks)) continue;
        for (let j = 0; j < hooks.length; j++) {
          const hook = hooks[j];
          const command = hook && hook.command;
          // Claude Code rewrites settings.json and its serializer drops `_source`, so requiring it reports a working hook as missing.
          if (typeof command === "string"
              && (!completion || (command.includes("CODETERM_SESSION_BINDING_NONCE")
                && command.includes('--data-binary "@-"')))
              && isCurrentHookCommand(command, host.platform())
              && command.indexOf("CODETERM_TAB_ID") !== -1
              && command.indexOf("CODETERM_API_URL") !== -1
              && command.indexOf("CODETERM_API_TOKEN") !== -1
              && command.indexOf("CODETERM_API_PORT") === -1
              && command.indexOf("CODETERM_PANE_ID") === -1) return true;
        }
      }
      return false;
    }
    // SessionStart must also carry the binding-nonce header, else it's a pre-nonce hook to force-upgrade.
    function sessionStartCurrent(): boolean {
      const arr = json!.hooks && json!.hooks.SessionStart;
      if (!Array.isArray(arr)) return false;
      for (let i = 0; i < arr.length; i++) {
        const hooks = arr[i] && arr[i].hooks;
        if (!Array.isArray(hooks)) continue;
        for (let j = 0; j < hooks.length; j++) {
          const h = hooks[j];
          const command = h && h.command;
          if (typeof command === "string"
              && isCurrentHookCommand(command, host.platform())
              && command.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1
              && command.indexOf("CODETERM_TAB_ID") !== -1
              && command.indexOf("CODETERM_API_URL") !== -1
              && command.indexOf("CODETERM_API_TOKEN") !== -1
              && command.indexOf("CODETERM_API_PORT") === -1
              && command.indexOf("CODETERM_PANE_ID") === -1) return true;
        }
      }
      return false;
    }
    const n = present("Notification");
    const pr = present("PermissionRequest");
    const s = present("Stop", true);
    const pc = Array.isArray(json.hooks?.PreCompact) && json.hooks.PreCompact.some(isCodetermHook);
    const ss = sessionStartCurrent();
    const installed = n && pr && s && ss;
    const owned = INTEGRATION_HOOK_KEYS.some((key) => {
      const arr = json.hooks && json.hooks[key];
      return Array.isArray(arr) && arr.some((entry) => {
        return entryCarriesMarker(entry) || entryIsLegacyPaneCodetermHook(entry);
      });
    });
    // False means the machine still runs pre-marker hooks the consented
    // reconciliation migrates once; it never means the hooks are broken.
    let marked = true;
    for (let i = 0; i < INTEGRATION_HOOK_KEYS.length; i++) {
      const arr = json.hooks && json.hooks[INTEGRATION_HOOK_KEYS[i]];
      if (Array.isArray(arr) && !entriesCarryMarker(arr)) { marked = false; break; }
    }
    return {
      settings_exists: true,
      hooks_installed: installed,
      integration_owned: owned,
      hook_contract_current: installed && marked && !pc,
      notification_hook: n,
      permission_request_hook: pr,
      stop_hook: s,

      session_start_hook: ss,
      target: path,
      backup_exists: host.fileExists(path + ".codeterm-backup"),
      proposed_changes: ["PermissionRequest", "Notification", "Stop", "SessionStart"],
    };
  },

  installIntegration(
    _apiPort: number,
    options?: { consent?: boolean; repair?: boolean },
  ): unknown {
    if (!options || options.consent !== true) {
      return { ok: false, error: "provider integration installation requires explicit local-user consent" };
    }
    const m = manifest();
    const path = host.expandHome((m && m.integration && m.integration.settingsPath)
      || "~/.claude/settings.json");
    if (!path) return { ok: false, error: "could not expand settingsPath" };

    const existed = host.fileExists(path);
    const originalValue = existed ? host.fs.readFile(path) : null;
    if (existed && typeof originalValue !== "string") {
      return { ok: false, error: "could not read existing settings JSON at " + path };
    }
    const original = typeof originalValue === "string" ? originalValue : "";
    let parsed: unknown = {};
    if (existed) {
      try {
        parsed = JSON.parse(original);
      } catch (_) {
        return { ok: false, error: "existing settings JSON is malformed at " + path };
      }
    }
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return { ok: false, error: "existing settings JSON must contain an object at " + path };
    }
    const json = parsed as ClaudeSettings;
    if (json.hooks !== undefined && (typeof json.hooks !== "object" || Array.isArray(json.hooks))) {
      return { ok: false, error: "hooks is not an object" };
    }
    if (!json.hooks) json.hooks = {};

    const executable = host.platform() === "windows" ? "curl.exe" : "curl";
    const hookCommand = (path: string, body: string, extraHeaders: string, stdin: boolean): string => {
      const payload = stdin ? '--data-binary "@-"' : `--data-binary "${body}"`;
      return `${executable} --silent --request POST "$CODETERM_API_URL${path}" `
        + `--header "Content-Type: application/json" --header "Authorization: Bearer $CODETERM_API_TOKEN" `
        + `${CODETERM_HOOK_MARKER_ARG} ${extraHeaders} ${payload}`;
    };
    const nonceHeader = '--header "X-CodeTerm-Binding-Nonce: $CODETERM_SESSION_BINDING_NONCE"';
    const notifyCmd = hookCommand("/api/tabs/$CODETERM_TAB_ID/hook/permission", "", nonceHeader, true);
    const stopCmd = hookCommand("/api/tabs/$CODETERM_TAB_ID/hook/stop", "", nonceHeader, true);
    const sessionStartCmd = hookCommand(
      "/api/tabs/$CODETERM_TAB_ID/hook/session_start",
      "",
      nonceHeader,
      true,
    );

    function newEntry(matcher: string, cmd: string, timeout: number): HookEntry {
      return {
        matcher: matcher,
        hooks: [{
          type: "command",
          command: cmd,
          timeout: timeout,
          _source: CODETERM_HOOK_MARKER,
        }],
      };
    }
    // PermissionRequest and Notification share the permission route and differ
    // only in matcher: per-tool vs the vendor's late permission_prompt.
    const newHooks: Record<string, HookEntry> = {
      PermissionRequest: newEntry("*", notifyCmd, 5000),
      Notification:      newEntry("permission_prompt", notifyCmd, 5000),
      Stop:              newEntry("", stopCmd, 5000),
      SessionStart:      newEntry("", sessionStartCmd, 5000),
    };

    // Remove existing codeterm hooks before appending to avoid stale leftovers.
    const keys = INTEGRATION_HOOK_KEYS;
    let preservedUnclassified = 0;
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (json.hooks[k] !== undefined && !Array.isArray(json.hooks[k])) {
        return { ok: false, error: "hooks." + k + " is not an array" };
      }
      const existing = Array.isArray(json.hooks[k])
        ? json.hooks[k].filter((entry) => {
          const retained = k === "PreCompact" ? retainForeignPrecompactEntry(entry) : !isCodetermHook(entry);
          if (retained) preservedUnclassified++;
          return retained;
        })
        : [];
      if (newHooks[k]) {
        existing.push(newHooks[k]);
        json.hooks[k] = existing;
      } else if (Array.isArray(json.hooks[k])) {
        json.hooks[k] = existing;
      }
    }

    if (!host.fs.writeFile(path + ".codeterm-backup", original)
      || !host.fs.writeFile(path + ".codeterm-backup-state", existed ? "present" : "absent")) {
      return { ok: false, error: "could not write recoverable backup for " + path };
    }
    const ok = host.fs.writeJson(path, json);
    if (ok) {
      if (options.repair === true && preservedUnclassified > 0) {
        host.log("warn", "[claude-plugin] repair preserved " + preservedUnclassified + " foreign or unclassifiable hook entries at " + path);
      }
      host.log("info", "[claude-plugin] hooks installed at " + path);
      return { ok: true };
    }
    return { ok: false, error: "writeJson failed for " + path };
  },

  uninstallIntegration(): unknown {
    const m = manifest();
    const path = host.expandHome((m && m.integration && m.integration.settingsPath)
      || "~/.claude/settings.json");
    if (!path || !host.fileExists(path)) return { ok: true };
    const original = host.fs.readFile(path);
    if (typeof original !== "string") return { ok: false, error: "could not safely read settings JSON at " + path };
    let parsed: unknown;
    try {
      parsed = JSON.parse(original);
    } catch (_) {
      return { ok: false, error: "existing settings JSON is malformed at " + path };
    }
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return { ok: false, error: "existing settings JSON must contain an object at " + path };
    }
    const json = parsed as ClaudeSettings;
    if (json.hooks === undefined) return { ok: true };
    if (!json.hooks || typeof json.hooks !== "object" || Array.isArray(json.hooks)) {
      return { ok: false, error: "hooks is not an object" };
    }
    const keys = INTEGRATION_HOOK_KEYS;
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (json.hooks[k] !== undefined && !Array.isArray(json.hooks[k])) {
        return { ok: false, error: "hooks." + k + " is not an array" };
      }
    }
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (Array.isArray(json.hooks[k])) {
        json.hooks[k] = json.hooks[k].filter((entry) => k === "PreCompact" ? retainForeignPrecompactEntry(entry) : !isCodetermHook(entry));
      }
    }
    const ok = host.fs.writeJson(path, json);
    if (ok) {
      host.log("info", "[claude-plugin] hooks removed from " + path);
      return { ok: true };
    }
    return { ok: false, error: "writeJson failed for " + path };
  },

  restoreIntegration(): unknown {
    const m = manifest();
    const path = host.expandHome((m && m.integration && m.integration.settingsPath)
      || "~/.claude/settings.json");
    if (!path) return { ok: false, error: "could not expand settingsPath" };
    const backup = path + ".codeterm-backup";
    if (!host.fileExists(backup)) return { ok: false, error: "no recoverable backup at " + backup };
    const original = host.fs.readFile(backup);
    if (typeof original !== "string") return { ok: false, error: "restore failed for " + path };
    const state = host.fs.readFile(path + ".codeterm-backup-state");
    const wasAbsent = state === "absent" || (typeof state !== "string" && original === "");
    if (wasAbsent) return !host.fileExists(path) || host.removeFile(path)
      ? { ok: true }
      : { ok: false, error: "restore failed for " + path };
    return host.fs.writeFile(path, original) ? { ok: true } : { ok: false, error: "restore failed for " + path };
  },

  // AS3/AS4 tab-state classification lives in ./classify; the facade delegates.
  // See that module for the auth/rate-limit/clarifying ensemble and its guarantees.
  classifyTabState(snapshot: TabStateSnapshot): TabStateClassification | null {
    return classifyClaudeTabState(snapshot);
  },

  detectLaunchSession(evidence: LaunchSessionEvidence): LaunchSessionDetection | null {
    return detectClaudeLaunchSession(evidence);
  },
};

function exactSourceIdentity(value: unknown): string | undefined {
  if (typeof value !== "string" || !value || value.trim() !== value) return undefined;
  return value;
}

function exactOutcomeNumber(value: unknown, nonnegative: boolean): number | undefined {
  if (typeof value !== "number" || !Number.isFinite(value) || !Number.isSafeInteger(value)) return undefined;
  if (nonnegative && value < 0) return undefined;
  return value;
}

function commandOutcome(value: unknown): Record<string, unknown> | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const result = value as ClaudeToolUseResult;
  const outcome: Record<string, unknown> = {};
  const exitCode = exactOutcomeNumber(result.returnCode, false);
  const durationMs = exactOutcomeNumber(result.durationMs, true);
  if (exitCode !== undefined) outcome.tool_exit_code = exitCode;
  if (durationMs !== undefined) outcome.tool_duration_ms = durationMs;
  if (typeof result.interrupted === "boolean") outcome.tool_interrupted = result.interrupted;
  const backgroundTaskId = exactSourceIdentity(result.backgroundTaskId);
  if (backgroundTaskId) outcome.tool_background_task_id = backgroundTaskId;
  return Object.keys(outcome).length > 0 ? outcome : undefined;
}

// Plain flag/value tokens stay bare; anything a shell could reinterpret is quoted.
function quoteArg(arg: string): string {
  return /^[A-Za-z0-9_\-.,:=@/+%]+$/.test(arg) ? arg : host.shell.quoteFor(arg, host.platform());
}


function resumeBase(sessionId: string, skipPermissions?: boolean): string {
    const win = host.platform() === "windows";
    const unset = win
      ? "Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; "
        + "$OutputEncoding = [Console]::OutputEncoding = [Console]::InputEncoding = "
        + "[System.Text.Encoding]::UTF8; "
        + "$PSNativeCommandArgumentPassing = 'Standard';"
      : "unset CLAUDECODE &&";
    const parts = [unset, "claude"];
    if (skipPermissions) parts.push("--dangerously-skip-permissions");
    parts.push("--resume");
    parts.push(host.shell.quoteFor(sessionId, host.platform()));

    return parts.join(" ");
}


export default plugin;
