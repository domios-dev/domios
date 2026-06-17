// Claude Code provider plugin (runtime logic, capability: agentProvider).
// Authored in TypeScript against @codeterm/plugin-sdk and compiled to a
// QuickJS-compatible plugin.js by scripts/build-plugin.mjs. Detection / parsing /
// slash commands live in plugin logic; host APIs cover side effects like bridge
// rule install (host.writeFile). The frontend ProviderPlugin contract is the
// separate src/index.ts artifact the app imports — not built into plugin.js.
import type { LaunchParams, PluginModule } from "@codeterm/plugin-sdk";

interface Manifest {
  rule?: { path?: string };
  integration?: { settingsPath?: string };
  sessions?: { rootDir?: string; fileExtension?: string };
}

function manifest(): Manifest | null {
  try { return JSON.parse(host.manifestJson() || "null") as Manifest; } catch (e) { return null; }
}

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

const TUI_FRAGMENTS = [
  "? for shortcuts",
  "for shortcuts",
  "ctrl+l",
  "esc to interrupt",
  // v2.1+ idle footer
  "bypass permissions",
  "shift+tab to cycle",
];

const SESSION_EXPLICIT_RE = /session(?:\s+id)?\s*[:=]\s*([a-z]+-[a-z]+-\d{2,6})\b/i;
const SESSION_BARE_RE = /(?:^|\n)\s*([a-z]{4,10}-[a-z]{4,10}-\d{4})\s*(?:$|\n)/m;
const CLAUDE_SESSION_DELTA_TYPES: Record<string, boolean> = { system: true, user: true, assistant: true, attachment: true };

function isBrailleSpinner(ch: string): boolean {
  if (!ch) return false;
  const code = ch.charCodeAt(0);
  return (code >= BRAILLE_LO && code <= BRAILLE_HI) || ch === "✳" || ch === "✻";
}

const plugin: PluginModule = {
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
    if (c.indexOf("proceed") !== -1) return true;
    if (c.indexOf("Esc to cancel") !== -1) return true;
    if (c.indexOf("Tab to amend") !== -1) return true;
    if (c.indexOf("1.") !== -1 && c.indexOf("2.") !== -1) return true;
    // "for 0..9" — task completion footers
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

  detectEvents(text: string): string[] {
    const t = String(text || "");
    const events: string[] = [];
    // PermissionRequest: numbered list + "Esc to cancel" or "❯ N." selector
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

  buildPromptResponse(optionIndex: number): string[] {
    // Claude TUI uses vim-style single-key navigation.
    const steps: string[] = [];
    for (let i = 0; i < optionIndex; i++) steps.push("ag==");
    steps.push("DQ==");
    return steps;
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
      // Quote each arg — they reach a shell, and `args` is caller-supplied via
      // the spawn API (a mesh peer with SpawnAgents could inject `; cmd`).
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    // Prompt is a positional, last, after `--`: a spawn task can start with the
    // -=-codeterm:...-=- origin marker, which commander would otherwise reject
    // as an unknown option.
    if (p.task) {
      const visiblePath = writeTaskTempFile(p.task);
      parts.push("--");
      parts.push(win
        ? "\"$(Get-Content '" + visiblePath + "' -Raw -Encoding UTF8)\""
        : "\"$(cat " + visiblePath + ")\"");
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

  // Parse the `/usage` TUI screen into a UsageSnapshot-shaped object
  // (snake_case fields, matching the Rust struct). Returns null when nothing
  // recognizable was found.
  parseUsage(rawText: string, nowMs: number): unknown {
    const raw = String(rawText || "");
    if (!raw.trim()) return null;
    const text = usageNormalize(raw);

    const session = usageParseBlock(text, "Current session", false, nowMs);
    const weekly = usageParseBlock(text, "Current week\\s*\\(\\s*all\\s*models\\s*\\)", true, nowMs);

    if (session.pct === null && weekly.pct === null) return null;
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

  // Rust provides complete lines; plugin parses JSON and builds RichMessage[].
  // Byte offsets stay in Rust.
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
      if (t === "system") {
        // System rows carry top-level `content` (string/array) + `subtype`.
        // The host normalizer turns them into system / local_command cards.
        role = "system";
        const sysText = (typeof v.content === "string")
          ? v.content
          : (Array.isArray(v.content)
              ? v.content.filter((c) => c && c.type === "text")
                  .map((c) => c.text || "").join("\n")
              : "");
        const sdata: Record<string, unknown> = { text: sysText };
        if (typeof v.subtype === "string") sdata.subtype = v.subtype;
        blocks.push({ kind: "text", data: sdata });
      } else if (t === "user" || t === "assistant") {
        role = t;
        const msg = v.message;
        if (!msg) continue;
        const content = msg.content;
        if (typeof content === "string") {
          if (content) blocks.push({ kind: "text", data: { text: content } });
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
            blocks.push({ kind: b.type, data: data });
          }
        } else {
          continue;
        }
      } else if (t === "attachment") {
        // Input that arrived while the agent was mid-generation is queued by
        // Claude as a queued_command attachment, not a normal user turn. Surface
        // it as the inbound user message it is, so a cross-pane send to a busy
        // pane still shows in chat instead of vanishing.
        const att = v.attachment;
        if (!att || att.type !== "queued_command" || typeof att.prompt !== "string" || !att.prompt) {
          continue;
        }
        role = "user";
        blocks.push({ kind: "text", data: { text: att.prompt } });
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
  // Hooks use `_source: "codeterm-hook-v2"` and manifest paths for stable lifecycle.

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
          if (hooks[j] && hooks[j]._source === CODETERM_HOOK_MARKER) return true;
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
              && h.command.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1) return true;
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
      hooks_installed: n && s && pc && ss,
      notification_hook: n,
      stop_hook: s,
      precompact_hook: pc,
      session_start_hook: ss,
    };
  },

  installIntegration(apiPort: number): unknown {
    const m = manifest();
    const path = host.expandHome((m && m.integration && m.integration.settingsPath)
      || "~/.claude/settings.json");
    if (!path) return { ok: false, error: "could not expand settingsPath" };

    const json = (host.fileExists(path) ? (host.fs.readJson<ClaudeSettings>(path) || {}) : {}) as ClaudeSettings;
    if (!json.hooks || typeof json.hooks !== "object") json.hooks = {};

    const nullDev = host.platform() === "windows" ? "NUL" : "/dev/null";
    const port = apiPort | 0;
    const authH = '-H "Authorization: Bearer ${CODETERM_API_TOKEN}"';
    const nonceH = '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"';
    const portExpr = "${CODETERM_API_PORT:-" + port + "}";

    const notifyCmd = "curl -s -X POST http://localhost:" + portExpr
      + "/api/panes/$CODETERM_PANE_ID/hook/permission "
      + "-H 'Content-Type: application/json' " + authH + " -d '{}' > " + nullDev + " 2>&1 || true";
    const stopCmd = "curl -s -X POST http://localhost:" + portExpr
      + "/api/panes/$CODETERM_PANE_ID/hook/stop "
      + "-H 'Content-Type: application/json' " + authH + " -d '{}' > " + nullDev + " 2>&1 || true";
    const precompactCmd = "curl -s -X POST http://localhost:" + portExpr
      + "/api/mem/flush -H 'Content-Type: application/json' " + authH
      + ' -d \'{"pane":"\'"$CODETERM_PANE_ID"\'","title":"pre-compact"}\' > '
      + nullDev + " 2>&1 || true";
    const sessionStartCmd = "curl -s -X POST http://localhost:" + portExpr
      + "/api/panes/$CODETERM_PANE_ID/hook/session_start "
      + "-H 'Content-Type: application/json' " + authH + " " + nonceH + " --data-binary @- > "
      + nullDev + " 2>&1 || true";

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
      PreCompact:    newEntry("", precompactCmd, 10000),
      SessionStart:  newEntry("", sessionStartCmd, 5000),
    };

    // Remove existing codeterm hooks before appending to avoid stale leftovers.
    const keys = ["Notification", "Stop", "PreCompact", "SessionStart"];
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (!Array.isArray(json.hooks[k])) json.hooks[k] = [];
      json.hooks[k] = json.hooks[k].filter((entry) => !isCodetermHook(entry));
      json.hooks[k].push(newHooks[k]);
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
    const json = host.fs.readJson<ClaudeSettings>(path);
    if (!json || !json.hooks || typeof json.hooks !== "object") return { ok: true };
    // Uninstall removes Notification + Stop only; keeps PreCompact + SessionStart.
    const keys = ["Notification", "Stop"];
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (!Array.isArray(json.hooks[k])) continue;
      json.hooks[k] = json.hooks[k].filter((entry) => !isCodetermHook(entry));
    }
    const ok = host.fs.writeJson(path, json);
    if (ok) {
      host.log("info", "[claude-plugin] hooks removed from " + path);
      return { ok: true };
    }
    return { ok: false, error: "writeJson failed for " + path };
  },
};

// --- types shared by the parsers above ---

interface ClaudeContentBlock { type?: string; text?: string; [k: string]: unknown }
interface ClaudeMessage { content?: string | ClaudeContentBlock[]; model?: string }
interface ClaudeEntry {
  type?: string;
  subtype?: string;
  content?: string | ClaudeContentBlock[];
  message?: ClaudeMessage;
  attachment?: { type?: string; prompt?: string };
  timestamp?: string;
  uuid?: string;
  isMeta?: boolean;
}
interface HookCmd { type?: string; command?: string; timeout?: number; _source?: string }
interface HookEntry { matcher?: string; hooks?: HookCmd[] }
interface ClaudeSettings { hooks?: Record<string, HookEntry[]> }

// Session-file/model-label helpers aligned with native `~/.claude/projects`.
// `encode_cwd_dashed` maps path separators to `-` and matches JsProvider path rules.
function claudeSessionFilePath(cwd: string, sessionId: string): string | null {
  const m = manifest();
  const root = (m && m.sessions && m.sessions.rootDir) || "~/.claude/projects";
  const ext = (m && m.sessions && m.sessions.fileExtension) || "jsonl";
  const expanded = host.expandHome(root);
  if (!expanded) return null;
  const encoded = encodeCwdDashed(cwd);
  const sep = host.platform() === "windows" ? "\\" : "/";
  return expanded + sep + encoded + sep + sessionId + "." + ext;
}

function encodeCwdDashed(cwd: string): string {
  // Match native behavior: map non-alphanumeric characters to '-'.
  return String(cwd || "").replace(/[^a-zA-Z0-9]/g, "-");
}

function contentToText(c: string | ClaudeContentBlock[] | undefined): string {
  if (typeof c === "string") return c;
  if (!Array.isArray(c)) return "";
  const chunks: string[] = [];
  for (let i = 0; i < c.length; i++) {
    const b = c[i];
    if (b && typeof b.text === "string") chunks.push(b.text);
  }
  return chunks.join(" ");
}

function isValidClaudeSessionMessage(v: ClaudeEntry): boolean {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type !== "user" && v.type !== "assistant") return false;
  if (!v.message || typeof v.message !== "object" || Array.isArray(v.message)) return false;
  const content = v.message.content;
  if (typeof content !== "string" && !Array.isArray(content)) return false;
  return true;
}

function validateEntry(v: ClaudeEntry): boolean {
  return isValidClaudeSessionDelta(v);
}

function isValidClaudeSessionDelta(v: ClaudeEntry): boolean {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (!CLAUDE_SESSION_DELTA_TYPES[v.type as string]) return false;
  if (v.type === "system") {
    const sc = v.content;
    return typeof sc === "string" || Array.isArray(sc);
  }
  if (v.type === "attachment") {
    const a = v.attachment;
    return !!a && typeof a === "object" && a.type === "queued_command" && typeof a.prompt === "string" && a.prompt.length > 0;
  }
  const msg = v.message;
  if (!msg || typeof msg !== "object" || Array.isArray(msg)) return false;
  if (typeof msg.content !== "string" && !Array.isArray(msg.content)) return false;
  return true;
}

function extractSetModelFamily(raw: string): string | null {
  const idx = raw.indexOf("Set model to ");
  if (idx === -1) return null;
  const after = raw.substring(idx + "Set model to ".length);
  const cleaned = stripAnsi(after).replace(/^\s+/, "");
  let fam = "";
  for (let i = 0; i < cleaned.length; i++) {
    const c = cleaned.charAt(i);
    if (!/[a-zA-Z]/.test(c)) break;
    fam += c;
  }
  return fam ? capitalize(fam) : null;
}

function stripAnsi(s: string): string {
  let out = "";
  let i = 0;
  while (i < s.length) {
    if (s.charCodeAt(i) === 0x1b && s.charAt(i + 1) === "[") {
      i += 2;
      while (i < s.length && !/[a-zA-Z]/.test(s.charAt(i))) i++;
      if (i < s.length) i++;
    } else {
      out += s.charAt(i);
      i++;
    }
  }
  return out;
}

function familyFromModelId(id: string): string {
  const m = String(id || "").toLowerCase();
  const fams = ["opus", "sonnet", "haiku"];
  for (let i = 0; i < fams.length; i++) {
    if (m.indexOf(fams[i]) !== -1) return capitalize(fams[i]);
  }
  const body = m.indexOf("claude-") === 0 ? m.substring(7) : m;
  const head = body.split("-")[0] || body;
  return capitalize(head);
}

function capitalize(s: string): string {
  if (!s) return "";
  return s.charAt(0).toUpperCase() + s.substring(1).toLowerCase();
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

// Hook recognition helpers.
const CODETERM_HOOK_MARKER = "codeterm-hook-v2";

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
        || cmd.indexOf("/api/panes/") !== -1
        || cmd.indexOf("/hook/permission") !== -1
        || cmd.indexOf("/hook/stop") !== -1
        || cmd.indexOf("/hook/session_start") !== -1
        || cmd.indexOf("/api/mem/flush") !== -1;
      if (isUrl && isPath) return true;
    }
  }
  return false;
}

// ── /usage screen parsing (port of the native usage_tui module) ─────────

function usageNormalize(raw: string): string {
  let out = "";
  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i];
    if (ch === "" || ch === "\r") continue;
    if ("█▌▍▎▏▊▋".indexOf(ch) !== -1) { out += " "; continue; }
    out += ch;
  }
  return out;
}

interface UsageBlock { pct: number | null; resetsMs: number | null }

function usageParseBlock(text: string, marker: string, isWeekly: boolean, nowMs: number): UsageBlock {
  const re = new RegExp(
    marker + "[\\s\\S]{0,800}?(\\d+(?:\\.\\d+)?)\\s*%\\s*used[\\s\\S]*?Resets?\\s*([^()\\n]+?)\\s*\\(",
  );
  const caps = re.exec(text);
  if (!caps) return { pct: null, resetsMs: null };
  let pct: number | null = parseFloat(caps[1]);
  if (isNaN(pct)) pct = null;
  const timeRaw = (caps[2] || "").trim();
  const resetsMs = isWeekly
    ? usageParseWeeklyReset(timeRaw, nowMs)
    : usageParseTodayClock(timeRaw, nowMs);
  return { pct: pct, resetsMs: resetsMs };
}

interface HMmAmPm { hh: number; mm: number; pm: boolean }

function usageParseHMmAmPm(s: string): HMmAmPm | null {
  s = String(s || "").trim().toLowerCase();
  let pm: boolean;
  if (s.slice(-2) === "pm") pm = true;
  else if (s.slice(-2) === "am") pm = false;
  else return null;
  const stripped = s.slice(0, -2).trim();
  let hh: number, mm: number;
  const idx = stripped.indexOf(":");
  if (idx !== -1) {
    hh = parseInt(stripped.slice(0, idx).trim(), 10);
    mm = parseInt(stripped.slice(idx + 1).trim(), 10);
  } else {
    hh = parseInt(stripped, 10);
    mm = 0;
  }
  if (isNaN(hh) || isNaN(mm) || hh === 0 || hh > 12 || mm > 59) return null;
  return { hh: hh, mm: mm, pm: pm };
}

function usageParseTodayClock(s: string, nowMs: number): number | null {
  const t = usageParseHMmAmPm(s);
  if (!t) return null;
  const hour24 = (t.hh % 12) + (t.pm ? 12 : 0);
  const now = new Date(nowMs);
  let cand = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hour24, t.mm, 0, 0);
  if (cand.getTime() <= nowMs) cand = new Date(cand.getTime() + 24 * 3600 * 1000);
  return cand.getTime();
}

function usageParseWeeklyReset(s: string, nowMs: number): number | null {
  return usageParseMonthDayClock(s, nowMs)
    ?? usageParseRelativeDayClock(s, nowMs)
    ?? usageParseTodayClock(s, nowMs);
}

const USAGE_MONTHS: Record<string, number> = {
  jan: 0, january: 0, feb: 1, february: 1, mar: 2, march: 2, apr: 3, april: 3,
  may: 4, jun: 5, june: 5, jul: 6, july: 6, aug: 7, august: 7,
  sep: 8, sept: 8, september: 8, oct: 9, october: 9, nov: 10, november: 10,
  dec: 11, december: 11,
};

function usageParseMonthDayClock(s: string, nowMs: number): number | null {
  const parts = usageCleanParts(s);
  while (parts.length > 0 && usageIsWeekday(parts[0])) parts.shift();
  if (parts.length < 3) return null;
  const month = USAGE_MONTHS[parts[0]];
  if (month === undefined) return null;
  const day = usageParseDayToken(parts[1]);
  if (day === null) return null;
  let idx = 2;
  let explicitYear: number | null = null;
  if (/^\d{4}$/.test(parts[idx] || "")) {
    explicitYear = parseInt(parts[idx], 10);
    idx++;
  }
  if (parts[idx] === "at") idx++;
  const t = usageParseHMmAmPm(parts.slice(idx).join(" "));
  if (!t) return null;
  const hour24 = (t.hh % 12) + (t.pm ? 12 : 0);
  const now = new Date(nowMs);
  const year = explicitYear ?? now.getFullYear();
  let cand = new Date(year, month, day, hour24, t.mm, 0, 0);
  if (explicitYear === null && cand.getTime() < nowMs) {
    cand = new Date(year + 1, month, day, hour24, t.mm, 0, 0);
  }
  return cand.getTime();
}

function usageParseRelativeDayClock(s: string, nowMs: number): number | null {
  const parts = usageCleanParts(s);
  const first = parts[0];
  let offsetDays: number;
  if (first === "today" || first === "tonight") offsetDays = 0;
  else if (first === "tomorrow") offsetDays = 1;
  else return null;
  let idx = 1;
  if (parts[idx] === "at") idx++;
  const t = usageParseHMmAmPm(parts.slice(idx).join(" "));
  if (!t) return null;
  const hour24 = (t.hh % 12) + (t.pm ? 12 : 0);
  const now = new Date(nowMs);
  return new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() + offsetDays,
    hour24,
    t.mm,
    0,
    0,
  ).getTime();
}

function usageCleanParts(s: string): string[] {
  return String(s || "")
    .trim()
    .toLowerCase()
    .replace(/,/g, " ")
    .split(/\s+/)
    .map((p) => p.replace(/[.;]+$/g, ""))
    .filter(Boolean);
}

function usageParseDayToken(s: string): number | null {
  const n = parseInt(String(s || "").replace(/(?:st|nd|rd|th)$/i, ""), 10);
  return Number.isFinite(n) && n >= 1 && n <= 31 ? n : null;
}

function usageIsWeekday(s: string): boolean {
  return /^(mon|monday|tue|tues|tuesday|wed|wednesday|thu|thur|thurs|thursday|fri|friday|sat|saturday|sun|sunday)$/.test(s);
}

export default plugin;
