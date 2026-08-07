// plugin.js by scripts/build-plugin.mjs. Frontend contract lives in src/index.ts.
import type { LaunchParams, ParsedPrompt, PluginModule, ResumeParams } from "@codeterm/plugin-sdk";
import { manifest } from "./shared";
import {
  fetchClaudeUsage,
  parseFetchedClaudeUsage,
  usageBlockIsComplete,
  usageNormalize,
  usageParseBlock,
} from "./usage";
import {
  ClaudeEntry,
  claudeSessionFilePath,
  contentToText,
  extractSetModelFamily,
  familyFromModelId,
  isValidClaudeSessionMessage,
  validateEntry,
} from "./session";
import { discoverAnthropicModels } from "./models";
import {
  classifyClaudeTabState,
  TabStateSnapshot,
  TabStateClassification,
} from "./classify";
import {
  buildClaudePromptResponse,
  parseClaudeLetteredPrompt,
  parseClaudeNumberedPrompt,
  parseClaudeWizardPrompt,
} from "./prompts";

const TITLE_RE = /claude/i;
// v2.1+ TUIs prefix the title with a braille spinner glyph or ✳/✻.
const BRAILLE_LO = 0x2801;
const BRAILLE_HI = 0x28ff;

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
  if (prompt === "no_starter") return undefined;
  if (prompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (prompt === "team_bootstrap") {
    if (!hostText) throw new Error("team_bootstrap requires host starterPromptText");
    return hostText;
  }
  return FILE_TASK_PROMPT;
}

function xmlTagBody(content: string, tag: string): string | null {
  const open = `<${tag}>`;
  const close = `</${tag}>`;
  const start = content.indexOf(open);
  if (start < 0) return null;
  const bodyStart = start + open.length;
  const end = content.indexOf(close, bodyStart);
  if (end < 0) return null;
  return content.slice(bodyStart, end).trim();
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
  return (code >= BRAILLE_LO && code <= BRAILLE_HI) || ch === "✳" || ch === "✻";
}


const plugin: PluginModule = {
  starterPromptText(prompt: "no_starter" | "assigned_task" | "idle_wakeup" | "team_bootstrap"): string | undefined {
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
    if (parseClaudeWizardPrompt(t)) events.push("permission_request");
    const numLines = (t.match(/^\s*\d+\.\s+/gm) || []).length;
    const hasEscFooter = t.indexOf("Esc to cancel") !== -1 || t.indexOf("Tab to amend") !== -1;
    const hasSelector = t.indexOf("❯") !== -1;
    if (numLines >= 2 && (hasEscFooter || hasSelector)) {
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

  parsePrompt(text: string): ParsedPrompt | null {
    return parseClaudeWizardPrompt(text) || parseClaudeNumberedPrompt(text) || parseClaudeLetteredPrompt(text);
  },

  buildPromptResponse(optionIndex: number, screen?: string): string[] {
    return buildClaudePromptResponse(optionIndex, String(screen || ""));
  },

  directoryTrustPromptResponse(prompt: ParsedPrompt, screen?: string): string[] | null {
    const question = String(prompt.question || "").toLowerCase();
    const first = prompt.options[0];
    const firstLabel = Array.isArray(first) ? first[0] : (first as unknown as { label?: string }).label;
    const isTrustGate = question.includes("trust") && question.includes("project");
    if (!isTrustGate || !firstLabel || !/^yes\b.*trust/i.test(firstLabel)) return null;
    return buildClaudePromptResponse(0, String(screen || ""));
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
    if (p.args && p.args.length > 0) {
      // Quote each caller-supplied arg — it reaches a shell (a mesh peer could inject `; cmd`).
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    // Keep large worker prompts out of shell parsing and argv on every platform.
    if (p.task) {
      const visiblePath = writeTaskTempFile(p.task);
      parts.push("--append-system-prompt-file");
      parts.push(host.shell.quoteFor(String(visiblePath), host.platform()));
      const prompt = starterPrompt(p.starterPrompt, p.starterPromptText);
      if (prompt) {
        parts.push("--");
        parts.push(host.shell.quoteFor(prompt, host.platform()));
      }
    }
    return parts.join(" ");
  },

  buildResumeCommand(sessionId: string, skipPermissions?: boolean): string {
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
  },

  // `--resume` restores the transcript but not the launch-time system prompt,
  // so it is re-applied through the same declared flag buildLaunchCommand uses.
  buildResumeCommandWithContext(params: ResumeParams): string {
    const p = params || ({} as ResumeParams);
    const parts = [this.buildResumeCommand(p.sessionId, p.skipPermissions)];
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) parts.push(String(p.args[i]));
    }
    const systemPrompt = p.systemPrompt;
    if (systemPrompt && systemPrompt.trim()) {
      const visiblePath = writeTaskTempFile(systemPrompt);
      if (visiblePath) {
        parts.push("--append-system-prompt-file");
        parts.push(host.shell.quoteFor(String(visiblePath), host.platform()));
      }
    }
    return parts.join(" ");
  },

  // Tail session JSONL to find latest model (`message.model` or `Set model to ...`).
  detectSessionModel(cwd: string, sessionId: string): string | null {
    const path = claudeSessionFilePath(cwd, sessionId);
    if (!path || !host.fs.fileExists(path)) return null;
    const contents = host.fs.readFile(path);
    if (!contents) return null;
    const lines = contents.split("\n");
    for (let i = lines.length - 1; i >= 0; i--) {
      const line = lines[i];
      if (!line) continue;
      let v: ClaudeEntry;
      try { v = JSON.parse(line); } catch (_) { continue; }
      if (!v || !v.message) continue;
      const raw = contentToText(v.message.content);
      const fam = extractSetModelFamily(raw);
      if (fam) return fam;
      const m = v.message.model;
      if (typeof m === "string" && m && m !== "<synthetic>") {
        return familyFromModelId(m);
      }
    }
    return null;
  },

  detectSessionReasoningEffort(cwd: string, sessionId: string): string | null {
    const path = claudeSessionFilePath(cwd, sessionId);
    if (!path || !host.fs.fileExists(path)) return null;
    const contents = host.fs.readFileTail(path, 4 * 1024 * 1024);
    if (!contents) return null;
    const supported: Record<string, boolean> = { low: true, medium: true, high: true, xhigh: true, max: true };
    const lines = contents.split("\n");
    for (let i = lines.length - 1; i >= 0; i--) {
      if (!lines[i]) continue;
      let value: Record<string, unknown>;
      try { value = JSON.parse(lines[i]) as Record<string, unknown>; } catch (_) { continue; }
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
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
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
      if (!validateEntry(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      const t = v && v.type;
      const blocks: Array<{ kind: string; data: Record<string, unknown> }> = [];
      let role: string;
      let sourceSubtype: string | null = (typeof v.subtype === "string") ? v.subtype : null;
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
          const taskStatus = t === "user" ? taskNotificationStatus(content) : null;
          if (taskStatus) {
            role = "status";
            sourceSubtype = "task_notification";
            blocks.push({ kind: "text", data: { text: taskStatus, subtype: sourceSubtype } });
          } else if (t === "user" && isCompactSummaryRow(v, content)) {
            role = "system";
            sourceSubtype = "compact_summary";
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
            if (b.type === "tool_use") {
              const canonical = canonicalToolFields(
                typeof data.name === "string" ? data.name : "",
                data.input,
              );
              data.tool_kind = canonical.kind;
              data.tool_summary = canonical.summary;
              data.tool_detail = canonical.detail;
              data.tool_edits = canonical.edits;
            }
            blocks.push({ kind: b.type, data: data });
          }
        } else {
          continue;
        }
      } else if (t === "attachment") {
        // Input sent while the agent was busy is queued as a queued_command attachment; surface it as the user message it is.
        const att = v.attachment;
        if (!att || att.type !== "queued_command" || typeof att.prompt !== "string" || !att.prompt) {
          continue;
        }
        const taskStatus = att.commandMode === "task-notification"
          ? taskNotificationStatus(att.prompt) ?? "Task notification"
          : taskNotificationStatus(att.prompt);
        if (taskStatus) {
          role = "status";
          sourceSubtype = "task_notification";
          blocks.push({ kind: "text", data: { text: taskStatus, subtype: sourceSubtype } });
        } else {
          role = "user";
          blocks.push({ kind: "text", data: { text: att.prompt } });
        }
      } else {
        continue;
      }
      if (!blocks.length) continue;
      let ts: number | null = null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1000);
      }
      out.push({
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
        timestamp: (typeof v.timestamp === "string") ? v.timestamp : null,
      });
    }
    return { messages: out };
  },

  // Keep JSONL path encoding aligned with `claudeSessionFilePath`.
  sessionJsonlPath(_cwd: string, sessionId: string): string | null {
    if (!sessionId) return null;
    return claudeSessionFilePath(_cwd, sessionId);
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

  // Manage Claude settings hooks for Notification, Stop, PreCompact, SessionStart.
  // Hooks use a versioned source marker and runtime callback identity.

  checkIntegration(): unknown {
    const m = manifest();
    const path = host.expandHome((m && m.integration && m.integration.settingsPath)
      || "~/.claude/settings.json");
    if (!path || !host.fileExists(path)) {
      return {
        settings_exists: false,
        hooks_installed: false,
        notification_hook: false,
        stop_hook: false,
        precompact_hook: false,
        session_start_hook: false,
        target: path || "~/.claude/settings.json",
        backup_exists: !!path && host.fileExists(path + ".codeterm-backup"),
        proposed_changes: ["Notification", "Stop", "SessionStart"],
      };
    }
    const json = host.fs.readJson<ClaudeSettings>(path);
    if (!json) {
      return { settings_exists: true, hooks_installed: false };
    }
    function present(key: string): boolean {
      const arr = json!.hooks && json!.hooks[key];
      if (!Array.isArray(arr)) return false;
      for (let i = 0; i < arr.length; i++) {
        const entry = arr[i];
        const hooks = entry && entry.hooks;
        if (!Array.isArray(hooks)) continue;
        for (let j = 0; j < hooks.length; j++) {
          const hook = hooks[j];
          if (hook && hook._source === CODETERM_HOOK_MARKER
              && typeof hook.command === "string"
              && hook.command.indexOf("CODETERM_TAB_ID") !== -1
              && hook.command.indexOf("CODETERM_API_URL") !== -1
              && hook.command.indexOf("CODETERM_API_TOKEN") !== -1
              && hook.command.indexOf("CODETERM_API_PORT") === -1
              && hook.command.indexOf("CODETERM_PANE_ID") === -1
              && hook.command.indexOf("/api/panes/") === -1) return true;
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
          if (h && h._source === CODETERM_HOOK_MARKER && typeof h.command === "string"
              && h.command.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1
              && h.command.indexOf("CODETERM_TAB_ID") !== -1
              && h.command.indexOf("CODETERM_API_URL") !== -1
              && h.command.indexOf("CODETERM_API_TOKEN") !== -1
              && h.command.indexOf("CODETERM_API_PORT") === -1
              && h.command.indexOf("CODETERM_PANE_ID") === -1
              && h.command.indexOf("/api/panes/") === -1) return true;
        }
      }
      return false;
    }
    const n = present("Notification");
    const s = present("Stop");
    const pc = present("PreCompact");
    const ss = sessionStartCurrent();
    return {
      settings_exists: true,
      hooks_installed: n && s && ss,
      notification_hook: n,
      stop_hook: s,
      precompact_hook: pc,
      session_start_hook: ss,
      target: path,
      backup_exists: host.fileExists(path + ".codeterm-backup"),
      proposed_changes: ["Notification", "Stop", "SessionStart"],
    };
  },

  installIntegration(
    _apiPort: number,
    options?: { consent?: boolean; precompact_memory_export?: boolean },
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

    const guard = 'if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then ';
    const authH = '-H "Authorization: Bearer $CODETERM_API_TOKEN"';
    const nonceH = '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"';
    const finish = " || true; fi";

    const notifyCmd = guard + 'curl -s -o /dev/null -X POST "$CODETERM_API_URL'
      + '/api/tabs/$CODETERM_TAB_ID/hook/permission" '
      + "-H 'Content-Type: application/json' " + authH + " -d '{}'" + finish;
    const stopCmd = guard + 'curl -s -o /dev/null -X POST "$CODETERM_API_URL'
      + '/api/tabs/$CODETERM_TAB_ID/hook/stop" '
      + "-H 'Content-Type: application/json' " + authH + " -d '{}'" + finish;
    const precompactCmd = guard + 'curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/mem/flush" '
      + "-H 'Content-Type: application/json' " + authH
      + ' -d \'{"tab":"\'"$CODETERM_TAB_ID"\'","title":"pre-compact"}\'' + finish;
    const sessionStartCmd = guard + 'curl -s -o /dev/null -X POST "$CODETERM_API_URL'
      + '/api/tabs/$CODETERM_TAB_ID/hook/session_start" '
      + "-H 'Content-Type: application/json' " + authH + " " + nonceH + " --data-binary @-" + finish;

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
    const newHooks: Record<string, HookEntry> = {
      Notification:  newEntry("permission_prompt", notifyCmd, 5000),
      Stop:          newEntry("", stopCmd, 5000),
      SessionStart:  newEntry("", sessionStartCmd, 5000),
    };
    if (options.precompact_memory_export === true) {
      newHooks.PreCompact = newEntry("", precompactCmd, 10000);
    }

    // Remove existing codeterm hooks before appending to avoid stale leftovers.
    const keys = ["Notification", "Stop", "PreCompact", "SessionStart"];
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (json.hooks[k] !== undefined && !Array.isArray(json.hooks[k])) {
        return { ok: false, error: "hooks." + k + " is not an array" };
      }
      const existing = Array.isArray(json.hooks[k])
        ? json.hooks[k].filter((entry) => !isCodetermHook(entry))
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
    const keys = ["Notification", "Stop", "PreCompact", "SessionStart"];
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (json.hooks[k] !== undefined && !Array.isArray(json.hooks[k])) {
        return { ok: false, error: "hooks." + k + " is not an array" };
      }
    }
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (Array.isArray(json.hooks[k])) {
        json.hooks[k] = json.hooks[k].filter((entry) => !isCodetermHook(entry));
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
};

interface CanonicalToolFields {
  kind: string;
  summary: string;
  detail: string;
  edits: Array<{ path: string; old: string; new: string }>;
}

function canonicalToolFields(name: string, input: unknown): CanonicalToolFields {
  const tool = name.trim();
  const value = input && typeof input === "object" && !Array.isArray(input)
    ? input as Record<string, unknown>
    : {};
  const text = (key: string): string => typeof value[key] === "string" ? value[key] as string : "";
  const command = text("command");
  const path = text("file_path") || text("filePath") || text("path");
  const pattern = text("pattern");
  const edits = extractCanonicalEdits(value);
  const lower = tool.toLowerCase();
  if (lower === "bash" || lower === "shell" || lower === "exec") {
    return { kind: "command", summary: cleanToolText(command, "Command"), detail: command, edits: [] };
  }
  if (lower === "edit" || lower === "multiedit" || lower === "write") {
    return { kind: "edit", summary: path || "File edit", detail: path, edits: edits };
  }
  if (lower === "read") {
    return { kind: "read", summary: path || "Read file", detail: path, edits: [] };
  }
  if (lower === "glob" || lower === "grep" || lower === "search") {
    return { kind: "search", summary: cleanToolText(pattern || path, "Search"), detail: pattern || path, edits: [] };
  }
  return {
    kind: "generic",
    summary: cleanToolText(tool, "Tool"),
    detail: tool,
    edits: edits,
  };
}

function extractCanonicalEdits(input: Record<string, unknown>): Array<{ path: string; old: string; new: string }> {
  const parentPath = typeof input.file_path === "string" ? input.file_path : typeof input.filePath === "string" ? input.filePath : typeof input.path === "string" ? input.path : "";
  const pair = (value: unknown, fallbackPath: string): { path: string; old: string; new: string } | null => {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const item = value as Record<string, unknown>;
    const oldValue = typeof item.old_string === "string" ? item.old_string : item.oldString;
    const newValue = typeof item.new_string === "string" ? item.new_string : item.newString;
    if (typeof oldValue !== "string" && typeof newValue !== "string") return null;
    const path = typeof item.file_path === "string" ? item.file_path : typeof item.filePath === "string" ? item.filePath : typeof item.path === "string" ? item.path : fallbackPath;
    return { path: path, old: typeof oldValue === "string" ? oldValue : "", new: typeof newValue === "string" ? newValue : "" };
  };
  if (Array.isArray(input.edits)) return input.edits.map((edit) => pair(edit, parentPath)).filter((e): e is { path: string; old: string; new: string } => e !== null);
  const single = pair(input, parentPath);
  return single ? [single] : [];
}

function cleanToolText(value: string, fallback: string): string {
  const text = String(value || "").replace(/[\r\n\t]+/g, " ").trim();
  if (!text) return fallback;
  return text.length > 120 ? text.slice(0, 117) + "..." : text;
}

// Task-file helpers: write to host temp files with provider-specific suffixes.
// Windows adds a UTF-8 BOM so PowerShell parses task bodies correctly.
function writeTaskTempFile(task: string): string | null {
  const suffix = ".ct-task-" + randomShortHex(8);
  const win = host.platform() === "windows";
  // PowerShell reads UTF-8 reliably only with BOM; prepend it for Windows task files.
  const body = win ? "﻿" + task : task;
  return host.fs.writeTempFile(body, suffix);
}

function randomShortHex(len: number): string {
  const hex = "0123456789abcdef";
  let out = "";
  for (let i = 0; i < len; i++) out += hex.charAt((Math.random() * 16) | 0);
  return out;
}

// Hook recognition helpers + types for the integration methods above.
interface HookCmd { type?: string; command?: string; timeout?: number; _source?: string }
interface HookEntry { matcher?: string; hooks?: HookCmd[] }
interface ClaudeSettings { hooks?: Record<string, HookEntry[]> }

const CODETERM_HOOK_MARKER = "codeterm-hook-v3";

function isCodetermHook(entry: HookEntry): boolean {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (let i = 0; i < hooks.length; i++) {
    const h = hooks[i];
    if (!h) continue;
    if (typeof h._source === "string"
        && (h._source.indexOf("codeterm-hook") === 0 || h._source === "codeterm")) {
      return true;
    }
    const cmd = h.command;
    if (typeof cmd === "string") {
      const isUrl = cmd.indexOf("7685") !== -1 || cmd.indexOf("7686") !== -1;
      const isPath = cmd.indexOf("/api/notifications") !== -1
        || cmd.indexOf("/api/tabs/") !== -1
        || cmd.indexOf("/hook/permission") !== -1
        || cmd.indexOf("/hook/stop") !== -1
        || cmd.indexOf("/hook/session_start") !== -1
        || cmd.indexOf("/api/mem/flush") !== -1;
      if (isUrl && isPath) return true;
    }
  }
  return false;
}

export default plugin;
