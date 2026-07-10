import type { LaunchParams, ModelInfo, ParsedPrompt, PluginModule } from "@codeterm/plugin-sdk";
import { discoverCodexModels } from "./models";
import { parseCodexPrompt, parseMenuOptions, responseKeysForIndex, type MenuOption } from "./prompt";
import { readSessionMeta } from "./sessionMeta";
import { parseCodexUsage } from "./usage";

const CODETERM_CODEX_HOOK_MARKER = "codeterm-codex-session-binding-v1";
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

const plugin: PluginModule = {
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
    const lower = String(screen || "").toLowerCase();
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    return false;
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

  // Codex CLI has no model-list subcommand; its local cache is the only sync account-scoped source. Missing => seed.
  discoverModels(): ModelInfo[] {
    return discoverCodexModels();
  },

  buildLaunchCommand(params: LaunchParams): string {
    const p = params || {};
    const parts: string[] = [];
    parts.push("codex");
    if (p.skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
    }
    if (p.args && p.args.length > 0) {
      // Quote each caller-supplied arg to prevent shell injection.
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    return parts.join(" ");
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
      if (stem === sessionId || extractSessionUuid(stem) === sessionId) {
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
        if (!hasUsefulFunctionCall(name, input, p.arguments)) continue;
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_use", data: { name: name || "tool", input: input } }],
          ts: ts,
          timestamp: tsIso,
        });
      } else if (p.type === "function_call_output") {
        let content: unknown = p.output;
        if (content && typeof content === "object") content = JSON.stringify(content);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_result", data: { content: String(content == null ? "" : content) } }],
          ts: ts,
          timestamp: tsIso,
        });
      } else if (p.type === "custom_tool_call") {
        const name = typeof p.name === "string" ? p.name.trim() : "";
        if (!name) continue;
        let input: Record<string, unknown> = {};
        if (name === "apply_patch" && typeof p.input === "string") {
          input = { patch: p.input };
        } else if (typeof p.input === "string") {
          try { input = JSON.parse(p.input); } catch (_) { input = { raw: p.input }; }
        } else if (p.input && typeof p.input === "object" && !Array.isArray(p.input)) {
          input = p.input as Record<string, unknown>;
        }
        const callId = typeof p.call_id === "string" ? p.call_id : "";
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_use", data: { name: name, id: callId, input: input } }],
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
      return { hooks_file_exists: false, hooks_installed: false, session_start_hook: false, hook_trust_state: "missing", hook_trust_enabled: null };
    }
    const json = host.fs.readJson<CodexHooks>(path);
    if (!json) {
      return { hooks_file_exists: true, hooks_installed: false, session_start_hook: false, hook_trust_state: "missing", hook_trust_enabled: null };
    }
    const arr = json.hooks && json.hooks.SessionStart;
    const installed = Array.isArray(arr) && arr.some(isCodetermCodexHook);
    const winScript = codexWindowsHookScriptPath();
    const windowsReady = host.platform() !== "windows" || (
      !!winScript &&
      host.fs.fileExists(winScript) &&
      Array.isArray(arr) &&
      arr.some(hasCodetermCodexWindowsCommand)
    );
    // A hook lacking the binding-nonce header is pre-nonce → not current, so startup reinstalls it.
    const nonceOk = host.platform() === "windows"
      ? (!!winScript && host.fs.fileExists(winScript) &&
         String(host.fs.readFile(winScript) || "").indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1)
      : (Array.isArray(arr) && arr.some((entry) => {
          const hk = entry && entry.hooks;
          return Array.isArray(hk) && hk.some((h) => {
            return !!h && String(h.command || "").indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1;
          });
        }));
    const current = installed && windowsReady && nonceOk;
    const trustState = codexHookTrustState(path);
    const trustOk = trustState !== "disabled";
    return {
      hooks_file_exists: true,
      hooks_installed: current && trustOk,
      session_start_hook: current && trustOk,
      hook_trust_state: trustState,
      hook_trust_enabled: trustState === "enabled" ? true : (trustState === "disabled" ? false : null),
    };
  },

  installIntegration(apiPort: number): unknown {
    const path = codexHooksPath();
    if (!path) return { ok: false, error: "could not resolve ~/.codex/hooks.json" };

    const json = (host.fs.fileExists(path) ? (host.fs.readJson<CodexHooks>(path) || {}) : {}) as CodexHooks;
    if (!json.hooks || typeof json.hooks !== "object") json.hooks = {};
    if (!Array.isArray(json.hooks.SessionStart)) json.hooks.SessionStart = [];
    json.hooks.SessionStart = json.hooks.SessionStart.filter((entry) => !isCodetermCodexHook(entry));

    const nullDev = host.platform() === "windows" ? "NUL" : "/dev/null";
    const port = apiPort | 0;
    const portExpr = "${CODETERM_API_PORT:-" + port + "}";
    const authH = '-H "Authorization: Bearer ${CODETERM_API_TOKEN}"';
    const nonceH = '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"';
    const cmd = "curl -s -X POST http://localhost:" + portExpr
      + "/api/panes/$CODETERM_PANE_ID/hook/session_start "
      + "-H 'Content-Type: application/json' "
      + "-H 'X-CodeTerm-Agent-Provider: codex' "
      + nonceH + " "
      + authH
      + " --data-binary @- > " + nullDev + " 2>&1 || true";
    const scriptPath = codexWindowsHookScriptPath();
    const commandWindows = scriptPath
      ? 'powershell -NoProfile -ExecutionPolicy Bypass -File "' + scriptPath + '"'
      : cmd;

    if (scriptPath && !host.fs.writeFile(scriptPath, codexWindowsHookScript(port))) {
      return { ok: false, error: "writeFile failed for " + scriptPath };
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
      host.log("info", "[codex-plugin] SessionStart hook installed at " + path);
      return { ok: true, hook_trust_state: trust };
    }
    return { ok: false, error: "writeJson failed for " + path };
  },

  uninstallIntegration(): unknown {
    const path = codexHooksPath();
    if (!path || !host.fs.fileExists(path)) return { ok: true };
    const json = host.fs.readJson<CodexHooks>(path);
    if (!json || !json.hooks || typeof json.hooks !== "object") return { ok: true };
    if (Array.isArray(json.hooks.SessionStart)) {
      json.hooks.SessionStart = json.hooks.SessionStart.filter((entry) => !isCodetermCodexHook(entry));
    }
    const ok = host.fs.writeJson(path, json);
    const scriptPath = codexWindowsHookScriptPath();
    if (scriptPath && host.fs.fileExists(scriptPath)) host.fs.removeFile(scriptPath);
    return ok ? { ok: true } : { ok: false, error: "writeJson failed for " + path };
  },

  ensureIntegrationOnStartup(apiPort: number): unknown {
    const status = this.checkIntegration!() as { hooks_installed?: boolean; hook_trust_state?: string };
    if (status && status.hooks_installed) return { ok: true };
    return this.installIntegration!(apiPort);
  },

  // Parse the `/status` TUI screen into a UsageSnapshot; gauges report "% left", snapshot stores "% used".
  parseUsage(rawText: string, nowMs: number): unknown {
    return parseCodexUsage(rawText, nowMs);
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

  // Read tail-most `turn_context.payload.model` and map to short model labels.
  detectSessionModel(cwd: string, sessionId: string): string | null {
    const path = this.sessionFilePath!(cwd, sessionId);
    if (!path) return null;
    const contents = host.fs.readFile(path);
    if (!contents) return null;
    const lines = contents.split("\n");
    for (let i = lines.length - 1; i >= 0; i--) {
      const line = lines[i];
      if (!line) continue;
      let v: CodexEntry;
      try { v = JSON.parse(line); } catch (_) { continue; }
      if (!v || v.type !== "turn_context") continue;
      const m = v.payload && v.payload.model;
      if (typeof m === "string" && m) return codexShortLabel(m);
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

  classifyPaneState(snapshot: PaneStateSnapshot): PaneStateClassification | null {
    // Normalize CRLF/CR → LF: the structural patterns below match on \n boundaries.
    const screen = String((snapshot && snapshot.screenText) || "").replace(/\r\n?/g, "\n");
    if (!screen.trim()) return null;

    // Auth first — sacred, takes precedence over any update-looking menu.
    const authConfidence = scoreAuthPrompt(screen);
    if (authConfidence >= CLASSIFY_THRESHOLD) {
      return {
        state: "waiting_on_auth",
        confidence: authConfidence,
        action: { surface: { kind: "auth_prompt", payload: { reason: "login_detected" } } },
      };
    }

    // Busy veto: a streaming pane is never parked at a limit screen.
    if (looksBusy(screen)) return { state: "working", confidence: 0 };

    // Model/effort picker veto: numbered menus without a limit notice must not schedule a resume.
    if (looksLikeModelPicker(screen)) return { state: "working", confidence: 0 };

    // Ensemble vote for the codex self-update prompt.
    const upd = scoreUpdatePrompt(screen);
    if (upd.confidence >= CLASSIFY_THRESHOLD && upd.skipKeys && upd.skipKeys.length > 0) {
      return {
        state: "update_prompt",
        confidence: upd.confidence,
        action: { keys: upd.skipKeys },
      };
    }

    // Usage-limit notice → schedule resume at the parsed reset (no menu to confirm).
    const rl = scoreRateLimit(screen, host.unixNowMs());
    if (rl.confidence >= CLASSIFY_THRESHOLD) {
      const action: PaneStateAction = {};
      const data: { resetAt?: string } = {};
      if (rl.resetAtMs !== null) {
        data.resetAt = new Date(rl.resetAtMs).toISOString();
        action.delayedResume = {
          at: new Date(rl.resetAtMs + RESUME_BUFFER_MS).toISOString(),
          text: "continue your work",
          thenKeys: ["enter"],
        };
      } else {
        action.surface = { kind: "rate_limit", payload: { reason: "reset_time_unparsed" } };
      }
      return {
        state: "rate_limited",
        confidence: rl.confidence,
        data: rl.resetAtMs !== null ? data : undefined,
        action: action,
      };
    }

    // Some signal but not a confident verdict → no action, so the reactor never fires on a partial read.
    const partial = rl.confidence > upd.confidence ? rl.confidence : upd.confidence;
    if (partial > 0) return { state: "unknown", confidence: partial };

    return { state: "working", confidence: 0 };
  },
};

// Pane-state classification (codex self-update prompt). Wire shapes mirror
// src-tauri/src/agent/pane_state.rs, declared locally because the SDK exposes

interface PaneStateSnapshot {
  paneId: string;
  agentType: string;
  sessionId?: string | null;
  cwd?: string | null;
  screenText: string;
}

type PaneState =
  | "working"
  | "waiting_on_auth"
  | "update_prompt"
  | "rate_limited"
  | "clarifying_question"
  | "idle_stuck"
  | "unknown";

interface PaneStateAction {
  keys?: string[];
  delayedResume?: { at: string; text: string; thenKeys?: string[] };
  surface?: { kind: string; payload: unknown };
}

interface PaneStateClassification {
  state: PaneState;
  confidence: number;
  data?: { resetAt?: string; questions?: string[] };
  action?: PaneStateAction;
}

// Must match the core reactor's GLOBAL_THRESHOLD (state_reactor.rs) — below it the reactor would reject the verdict.
const CLASSIFY_THRESHOLD = 0.8;

// Buffer past the parsed reset so `continue your work` lands after the limit actually clears.
const RESUME_BUFFER_MS = 60 * 1000;

// there's no distinct Skip option, so we never blindly Enter on the default "Update now".
function skipNavigationKeys(options: MenuOption[]): string[] | null {
  if (options.length === 0) return null;
  let highlightIdx = -1;
  for (let i = 0; i < options.length; i++) {
    if (options[i].highlighted) { highlightIdx = i; break; }
  }
  if (highlightIdx === -1) highlightIdx = 0; // codex defaults to option 1
  let skipIdx = -1;
  for (let i = 0; i < options.length; i++) {
    const label = options[i].label;
    if (/\bskip\b/i.test(label) && !/\bupdate\b/i.test(label)) { skipIdx = i; break; }
  }
  if (skipIdx === -1) return null;
  const keys: string[] = [];
  const delta = skipIdx - highlightIdx;
  if (delta > 0) for (let i = 0; i < delta; i++) keys.push("down");
  else if (delta < 0) for (let i = 0; i < -delta; i++) keys.push("up");
  keys.push("enter");
  return keys;
}

interface UpdateScore { confidence: number; skipKeys: string[] | null }

// signal is a numbered menu carrying both "update" and "skip"; a lone signal stays sub-threshold.
function scoreUpdatePrompt(screen: string): UpdateScore {
  const options = parseMenuOptions(screen);
  let hasUpdateOption = false;
  let hasSkipOption = false;
  for (let i = 0; i < options.length; i++) {
    if (/\bupdate\b/i.test(options[i].label)) hasUpdateOption = true;
    if (/\bskip\b/i.test(options[i].label)) hasSkipOption = true;
  }
  const sigUpdateSkipMenu = hasUpdateOption && hasSkipOption;          // 0.45
  const sigAvailable = /\b(update|version)\s+available\b/i.test(screen); // 0.20
  const sigVersionArrow = /\d+\.\d+(?:\.\d+)?\s*(?:->|→|=>)\s*\d+\.\d+(?:\.\d+)?/.test(screen); // 0.15
  const sigPressEnter = /press\s+enter\s+to\s+continue/i.test(screen); // 0.10
  const sigReleaseNotes = /release\s+notes/i.test(screen);             // 0.10

  let confidence = 0;
  if (sigUpdateSkipMenu) confidence += 0.45;
  if (sigAvailable) confidence += 0.2;
  if (sigVersionArrow) confidence += 0.15;
  if (sigPressEnter) confidence += 0.1;
  if (sigReleaseNotes) confidence += 0.1;
  if (confidence > 1) confidence = 1;

  // Only compute Skip navigation when the actionable menu signal is present.
  const skipKeys = sigUpdateSkipMenu ? skipNavigationKeys(options) : null;
  return { confidence: confidence, skipKeys: skipKeys };
}

function looksBusy(screen: string): boolean {
  const s = String(screen || "");
  return (
    /\bctrl\+c to (?:cancel|stop|interrupt|quit)\b/i.test(s) ||
    /\bWorking[….]/.test(s) ||
    /\((?:esc|ctrl\+c) to interrupt\)/i.test(s)
  );
}

function looksLikeModelPicker(screen: string): boolean {
  const s = String(screen || "");
  if (!/\bselect\s+(?:model|reasoning)\b/i.test(s)) return false;
  return /\bpress\s+enter\s+to\s+confirm\b/i.test(s) && /\besc\s+to\s+go\s+back\b/i.test(s);
}

interface RateLimitScore { confidence: number; resetAtMs: number | null }

// Structural signals for codex's plain-text usage-limit notice (weights sum to 1.0).
function scoreRateLimit(screen: string, nowMs: number): RateLimitScore {
  const s = String(screen || "");
  const sigHitLimit = /\bhit your usage limit\b/i.test(s) || /\breached your usage limit\b/i.test(s); // 0.50
  const sigTryAgain = /\btry again at\b/i.test(s);                                                   // 0.30
  const sigSettingsLink = /chatgpt\.com\/codex\/settings/i.test(s);                                   // 0.10
  const sigResetClock = /\btry again at\b[^\n]{0,48}?\d{1,2}(?::\d{2})?\s*(?:am|pm)\b/i.test(s);    // 0.10

  let confidence = 0;
  if (sigHitLimit) confidence += 0.5;
  if (sigTryAgain) confidence += 0.3;
  if (sigSettingsLink) confidence += 0.1;
  if (sigResetClock) confidence += 0.1;
  if (confidence > 1) confidence = 1;

  const resetAtMs = parseTryAgainClock(s, nowMs);
  return { confidence: confidence, resetAtMs: resetAtMs };
}

function parseTryAgainClock(screen: string, nowMs: number): number | null {
  const m = /\btry again at\b[^\n]{0,48}?(\d{1,2}(?::\d{2})?\s*(?:am|pm))\b/i.exec(String(screen || ""));
  if (!m) return null;
  return codexParseTodayClock(m[1], nowMs);
}

interface HMmAmPm { hh: number; mm: number; pm: boolean }

function codexParseHMmAmPm(s: string): HMmAmPm | null {
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

function codexParseTodayClock(s: string, nowMs: number): number | null {
  const t = codexParseHMmAmPm(s);
  if (!t) return null;
  const hour24 = (t.hh % 12) + (t.pm ? 12 : 0);
  const now = new Date(nowMs);
  let cand = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hour24, t.mm, 0, 0);
  if (cand.getTime() <= nowMs) cand = new Date(cand.getTime() + 24 * 3600 * 1000);
  return cand.getTime();
}

// Auth ensemble, weighted so no lone casual mention fires. Leans inclusive: a false positive only surfaces, never auto-acts.
function scoreAuthPrompt(screen: string): number {
  const s = String(screen || "");
  let score = 0;
  if (/\bsign[ -]?in\b/i.test(s)) score += 0.5;
  if (/\blog[ -]?in\b/i.test(s)) score += 0.5;
  if (/\bauthenticat(?:e|ion|ing)\b/i.test(s)) score += 0.5;
  if (/\bo\s?auth\b/i.test(s)) score += 0.6;
  if (/\bapi key\b/i.test(s)) score += 0.6;
  if (/paste\b[^\n]*\b(token|key|code|credential)/i.test(s)) score += 0.6;
  if (/\bauthorization code\b/i.test(s)) score += 0.6;
  if (/enter your\b[^\n]*\b(api key|token|credential)/i.test(s)) score += 0.6;
  return score > 1 ? 1 : score;
}

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
interface CodexHookEntry { matcher?: string; hooks?: CodexHookCmd[] }
interface CodexHooks { hooks?: { SessionStart?: CodexHookEntry[] } }

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

function codexWindowsHookScript(apiPort: number): string {
  return [
    "# " + CODETERM_CODEX_HOOK_MARKER,
    '$ErrorActionPreference = "SilentlyContinue"',
    "$pane = $env:CODETERM_PANE_ID",
    "if ([string]::IsNullOrWhiteSpace($pane)) { exit 0 }",
    '$port = if ($env:CODETERM_API_PORT) { $env:CODETERM_API_PORT } else { "' + String(apiPort || 7686) + '" }',
    '$base = if ($env:CODETERM_API_URL) { $env:CODETERM_API_URL.TrimEnd("/") } else { "http://127.0.0.1:$port" }',
    "$headers = @{",
    '  "Content-Type" = "application/json"',
    '  "X-CodeTerm-Agent-Provider" = "codex"',
    "}",
    "if ($env:CODETERM_SESSION_BINDING_NONCE) {",
    '  $headers["X-CodeTerm-Binding-Nonce"] = $env:CODETERM_SESSION_BINDING_NONCE',
    "}",
    "if ($env:CODETERM_API_TOKEN) {",
    '  $headers["Authorization"] = "Bearer $($env:CODETERM_API_TOKEN)"',
    "}",
    "$body = [Console]::In.ReadToEnd()",
    'Invoke-RestMethod -Method Post -Uri "$base/api/panes/$pane/hook/session_start" -Headers $headers -Body $body -TimeoutSec 5 | Out-Null',
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

function codexHookTrustState(hooksPath: string): "enabled" | "disabled" | "missing" | "unknown" {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return "missing";
  const text = String(host.fs.readFile(cfg) || "");
  if (!text) return "missing";
  const block = findCodexHookTrustBlock(text, hooksPath);
  if (block == null) return "missing";
  const m = block.match(/(?:^|\n)\s*enabled\s*=\s*(true|false)\s*(?:\n|$)/);
  if (!m) return "unknown";
  return m[1] === "true" ? "enabled" : "disabled";
}

function enableExistingCodexHookTrust(hooksPath: string): "enabled" | "missing" | "unknown" {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return "missing";
  const text = String(host.fs.readFile(cfg) || "");
  if (!text) return "missing";
  const span = findCodexHookTrustBlockSpan(text, hooksPath);
  if (!span) return "missing";
  const block = text.substring(span.start, span.end);
  if (!/(?:^|\n)\s*enabled\s*=\s*false\s*(?:\n|$)/.test(block)) {
    return codexHookTrustState(hooksPath) === "enabled" ? "enabled" : "unknown";
  }
  const nextBlock = block.replace(/(^|\n)(\s*enabled\s*=\s*)false(\s*(?:\n|$))/, "$1$2true$3");
  const next = text.substring(0, span.start) + nextBlock + text.substring(span.end);
  return host.fs.writeFile(cfg, next) ? "enabled" : "unknown";
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

// Shorten common model ids for tab badges (Codex / GPT / Sonnet etc.).
function codexShortLabel(modelId: string): string {
  const m = String(modelId || "").toLowerCase();
  if (m.indexOf("codex") !== -1) return "Codex";
  const fams = ["opus", "sonnet", "haiku", "gemini"];
  for (let i = 0; i < fams.length; i++) {
    if (m.indexOf(fams[i]) !== -1) {
      return fams[i].charAt(0).toUpperCase() + fams[i].substring(1);
    }
  }
  if (m.indexOf("gpt") === 0) {
    const head = m.split("-").slice(0, 2).join("-");
    return "GPT" + head.substring(3);
  }
  const first = m.split("-")[0] || m;
  return first.charAt(0).toUpperCase() + first.substring(1);
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
    kind !== "custom_tool_call"
  ) {
    return false;
  }
  return true;
}

function validateEntry(v: CodexEntry): boolean {
  return isValidCodexSessionDelta(v);
}

function hasUsefulFunctionCall(name: string, input: Record<string, unknown>, args: string | undefined): boolean {
  if (name) return true;
  if (typeof args === "string" && args.trim() && args.trim() !== "{}") return true;
  return Object.keys(input || {}).length > 0;
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
