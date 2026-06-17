"use strict";
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// plugins/provider-codex/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default
});
module.exports = __toCommonJS(plugin_exports);
var CODETERM_CODEX_HOOK_MARKER = "codeterm-codex-session-binding-v1";
var TITLE_RE = /codex/i;
var OUTPUT_FINGERPRINTS = [
  "OpenAI Codex",
  "permissions: YOLO mode",
  "permissions: read-only",
  "Would you like to run the following command",
  "Would you like to make the following edits",
  "approve network access",
  "Would you like to grant these permissions",
  "codex>"
];
var TUI_FRAGMENTS = ["openai codex", "ctrl+c to cancel"];
var plugin = {
  detectFromTitle(title) {
    return TITLE_RE.test(String(title || ""));
  },
  detectFromOutput(text) {
    const t = String(text || "");
    for (let i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
      if (t.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) return true;
    }
    return false;
  },
  screenHasTui(screen) {
    const lower = String(screen || "").toLowerCase();
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    return false;
  },
  isOutputNoise(text) {
    const trimmed = String(text || "").trim();
    if (!trimmed) return true;
    for (let i = 0; i < trimmed.length; i++) {
      const c = trimmed[i];
      if (c !== "\u2500" && c !== "\u2501" && c !== "\u2550" && c !== " " && c !== "	" && c !== "\n" && c !== "\r") {
        return false;
      }
    }
    return true;
  },
  hasEventMarkers(chunk) {
    const c = String(chunk || "");
    return c.indexOf("Would you like") !== -1 || c.indexOf("approve") !== -1 || c.indexOf("run the following") !== -1 || c.indexOf("make the following") !== -1 || c.indexOf("grant these permissions") !== -1;
  },
  detectEvents(text) {
    const t = String(text || "");
    const events = [];
    if (t.indexOf("Would you like to run the following command") !== -1 || t.indexOf("Would you like to make the following edits") !== -1 || t.indexOf("Do you want to approve network access") !== -1 || t.indexOf("Would you like to grant these permissions") !== -1) {
      events.push("permission_request");
    }
    return events;
  },
  buildLaunchCommand(params) {
    const p = params || {};
    const parts = [];
    parts.push("codex");
    parts.push("--dangerously-bypass-hook-trust");
    if (p.skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
    }
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    return parts.join(" ");
  },
  buildResumeCommand(sessionId, skipPermissions) {
    const parts = ["codex", "resume"];
    parts.push("--dangerously-bypass-hook-trust");
    if (skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
    }
    const uuid = extractSessionUuid(sessionId) || sessionId;
    parts.push(host.shell.quote(uuid));
    return parts.join(" ");
  },
  // Rollout-tree detection: scan `~/.codex/sessions/.../rollout-*.jsonl`
  // and read only `session_meta` headers on first file match.
  sessionFilePath(_cwd, sessionId) {
    const root = codexSessionsRoot();
    if (!root) return null;
    let hit = null;
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
  sessionJsonlPath(cwd, sessionId) {
    return this.sessionFilePath(cwd, sessionId);
  },
  // Duplicated by design across provider bundles (QuickJS, no imports) — parity guarded by provider-helper-parity.test.ts. Change both or the test fails.
  // Parse codex rollout JSONL into RichMessages. Codex schema:
  //   {type:"response_item", payload:{type:"message"|"function_call"|"function_call_output", ...}}
  // We read response_item lines only (event_msg duplicates them; reasoning is
  // encrypted; token_count/turn_context/session_meta are metadata).
  parseSessionDelta(chunk) {
    const out = [];
    const lines = String(chunk || "").split("\n");
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
      if (!lines[i]) continue;
      let v;
      try {
        v = JSON.parse(lines[i]);
      } catch (_) {
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
      if (!v || v.type !== "response_item" || !v.payload) continue;
      const p = v.payload;
      let ts = null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1e3);
      }
      if (p.type === "message") {
        const role = p.role === "assistant" ? "assistant" : p.role === "user" ? "user" : null;
        if (!role) continue;
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
        out.push({ role, blocks: [{ kind: "text", data: { text } }], ts });
      } else if (p.type === "function_call") {
        let input = {};
        try {
          input = JSON.parse(p.arguments || "{}");
        } catch (_) {
          input = { raw: p.arguments };
        }
        if (input && input.cmd && !input.command) input.command = input.cmd;
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_use", data: { name: p.name || "tool", input } }],
          ts
        });
      } else if (p.type === "function_call_output") {
        let content = p.output;
        if (content && typeof content === "object") content = JSON.stringify(content);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_result", data: { content: String(content == null ? "" : content) } }],
          ts
        });
      }
    }
    return { messages: out };
  },
  detectSessionId(_cwd, _exclude, _maxAgeMs) {
    return null;
  },
  checkIntegration() {
    const path = codexHooksPath();
    if (!path || !host.fs.fileExists(path)) {
      return { hooks_file_exists: false, hooks_installed: false, session_start_hook: false };
    }
    const json = host.fs.readJson(path);
    if (!json) {
      return { hooks_file_exists: true, hooks_installed: false, session_start_hook: false };
    }
    const arr = json.hooks && json.hooks.SessionStart;
    const installed = Array.isArray(arr) && arr.some(isCodetermCodexHook);
    const winScript = codexWindowsHookScriptPath();
    const windowsReady = host.platform() !== "windows" || !!winScript && host.fs.fileExists(winScript) && Array.isArray(arr) && arr.some(hasCodetermCodexWindowsCommand);
    const nonceOk = host.platform() === "windows" ? !!winScript && host.fs.fileExists(winScript) && String(host.fs.readFile(winScript) || "").indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1 : Array.isArray(arr) && arr.some((entry) => {
      const hk = entry && entry.hooks;
      return Array.isArray(hk) && hk.some((h) => {
        return !!h && String(h.command || "").indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1;
      });
    });
    return {
      hooks_file_exists: true,
      hooks_installed: installed && windowsReady && nonceOk,
      session_start_hook: installed && windowsReady && nonceOk
    };
  },
  installIntegration(apiPort) {
    const path = codexHooksPath();
    if (!path) return { ok: false, error: "could not resolve ~/.codex/hooks.json" };
    const json = host.fs.fileExists(path) ? host.fs.readJson(path) || {} : {};
    if (!json.hooks || typeof json.hooks !== "object") json.hooks = {};
    if (!Array.isArray(json.hooks.SessionStart)) json.hooks.SessionStart = [];
    json.hooks.SessionStart = json.hooks.SessionStart.filter((entry) => !isCodetermCodexHook(entry));
    const nullDev = host.platform() === "windows" ? "NUL" : "/dev/null";
    const port = apiPort | 0;
    const portExpr = "${CODETERM_API_PORT:-" + port + "}";
    const authH = '-H "Authorization: Bearer ${CODETERM_API_TOKEN}"';
    const nonceH = '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"';
    const cmd = "curl -s -X POST http://localhost:" + portExpr + "/api/panes/$CODETERM_PANE_ID/hook/session_start -H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' " + nonceH + " " + authH + " --data-binary @- > " + nullDev + " 2>&1 || true";
    const scriptPath = codexWindowsHookScriptPath();
    const commandWindows = scriptPath ? 'powershell -NoProfile -ExecutionPolicy Bypass -File "' + scriptPath + '"' : cmd;
    if (scriptPath && !host.fs.writeFile(scriptPath, codexWindowsHookScript(port))) {
      return { ok: false, error: "writeFile failed for " + scriptPath };
    }
    json.hooks.SessionStart.push({
      matcher: "startup|resume|clear|compact",
      hooks: [{
        type: "command",
        command: cmd,
        commandWindows,
        timeout: 5,
        _source: CODETERM_CODEX_HOOK_MARKER
      }]
    });
    const ok = host.fs.writeJson(path, json);
    if (ok) {
      host.log("info", "[codex-plugin] SessionStart hook installed at " + path);
      return { ok: true };
    }
    return { ok: false, error: "writeJson failed for " + path };
  },
  uninstallIntegration() {
    const path = codexHooksPath();
    if (!path || !host.fs.fileExists(path)) return { ok: true };
    const json = host.fs.readJson(path);
    if (!json || !json.hooks || typeof json.hooks !== "object") return { ok: true };
    if (Array.isArray(json.hooks.SessionStart)) {
      json.hooks.SessionStart = json.hooks.SessionStart.filter((entry) => !isCodetermCodexHook(entry));
    }
    const ok = host.fs.writeJson(path, json);
    const scriptPath = codexWindowsHookScriptPath();
    if (scriptPath && host.fs.fileExists(scriptPath)) host.fs.removeFile(scriptPath);
    return ok ? { ok: true } : { ok: false, error: "writeJson failed for " + path };
  },
  ensureIntegrationOnStartup(apiPort) {
    const status = this.checkIntegration();
    if (status && status.hooks_installed) return { ok: true };
    return this.installIntegration(apiPort);
  },
  // Parse the `/status` TUI screen into a UsageSnapshot-shaped object
  // (snake_case fields). Gauges report "% left"; snapshot stores "% used".
  parseUsage(rawText, nowMs) {
    const raw = String(rawText || "");
    if (!raw.trim()) return null;
    const text = raw.split("\r").join("").split("\u2588").join(" ").split("\u2591").join(" ");
    const session = codexParseGauge(text, "5h\\s*limit", false, nowMs);
    const weekly = codexParseGauge(text, "Weekly\\s*limit", true, nowMs);
    if (session.pct === null && weekly.pct === null) return null;
    return {
      provider: "codex",
      account_id: null,
      captured_at_ms: nowMs,
      session_pct: session.pct,
      session_resets_at_ms: session.resetsMs,
      weekly_pct: weekly.pct,
      weekly_resets_at_ms: weekly.resetsMs,
      raw_text: raw
    };
  },
  detectActiveSession(_cwd, _currentSid) {
    return null;
  },
  enumerateSessions() {
    const root = codexSessionsRoot();
    if (!root || !host.fs.fileExists(root)) return [];
    const out = [];
    const files = host.fs.walkDir(root, 5);
    for (let i = 0; i < files.length; i++) {
      const f = files[i];
      if (!f.isFile || !endsWith(f.name, ".jsonl")) continue;
      const stem = f.name.substring(0, f.name.length - 6);
      const meta = readSessionMeta(f.path);
      const sid = meta && meta.sessionId || extractSessionUuid(stem);
      if (!sid) continue;
      out.push({
        sessionId: sid,
        provider: "codex",
        projectPath: meta && meta.cwd ? meta.cwd : null,
        title: null,
        summary: null,
        filePath: f.path,
        createdTs: f.createdMs ? Math.floor(f.createdMs / 1e3) : null,
        modifiedTs: f.modifiedMs ? Math.floor(f.modifiedMs / 1e3) : null
      });
    }
    return out;
  },
  // Read tail-most `turn_context.payload.model` and map to short model labels.
  detectSessionModel(cwd, sessionId) {
    const path = this.sessionFilePath(cwd, sessionId);
    if (!path) return null;
    const contents = host.fs.readFile(path);
    if (!contents) return null;
    const lines = contents.split("\n");
    for (let i = lines.length - 1; i >= 0; i--) {
      const line = lines[i];
      if (!line) continue;
      let v;
      try {
        v = JSON.parse(line);
      } catch (_) {
        continue;
      }
      if (!v || v.type !== "turn_context") continue;
      const m = v.payload && v.payload.model;
      if (typeof m === "string" && m) return codexShortLabel(m);
    }
    return null;
  },
  // Mirror native parse_session_messages for response_item message blocks.
  parseSessionMessages(text) {
    const out = [];
    const lines = String(text || "").split("\n");
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      if (!line) continue;
      let v;
      try {
        v = JSON.parse(line);
      } catch (e) {
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
      const chunks = [];
      for (let j = 0; j < content.length; j++) {
        const b = content[j];
        if (!b) continue;
        if ((b.type === "input_text" || b.type === "output_text" || b.type === "text") && typeof b.text === "string") {
          chunks.push(b.text);
        }
      }
      const text2 = chunks.join(" ");
      if (!text2.trim()) continue;
      let ts = null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1e3);
      }
      out.push({ role, text: text2, ts, isMeta: false });
    }
    return out;
  }
};
function endsWith(s, suffix) {
  return s.length >= suffix.length && s.substring(s.length - suffix.length) === suffix;
}
function codexSessionsRoot() {
  const override = host.envGet("CODETERM_CODEX_SESSIONS_ROOT");
  if (override) return override;
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "sessions";
}
function codexHooksPath() {
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "hooks.json";
}
function codexWindowsHookScriptPath() {
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "codeterm-session-start.ps1";
}
function codexWindowsHookScript(apiPort) {
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
    ""
  ].join("\n");
}
function isCodetermCodexHook(entry) {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (let i = 0; i < hooks.length; i++) {
    const hook = hooks[i];
    if (!hook) continue;
    if (hook._source === CODETERM_CODEX_HOOK_MARKER) return true;
    const cmd = String(hook.command || "");
    if (cmd.indexOf("/hook/session_start") !== -1 && cmd.indexOf("X-CodeTerm-Agent-Provider: codex") !== -1) {
      return true;
    }
  }
  return false;
}
function hasCodetermCodexWindowsCommand(entry) {
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
function extractSessionUuid(stem) {
  if (typeof stem !== "string") return null;
  const prefix = "rollout-";
  if (stem.indexOf(prefix) !== 0) return null;
  const rest = stem.substring(prefix.length);
  if (rest.length <= 20) return null;
  if (rest.charAt(19) !== "-") return null;
  return rest.substring(20);
}
function codexShortLabel(modelId) {
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
function readSessionMeta(path) {
  const contents = host.fs.readFile(path);
  if (!contents) return null;
  const firstNl = contents.indexOf("\n");
  const line = firstNl === -1 ? contents : contents.substring(0, firstNl);
  let v;
  try {
    v = JSON.parse(line.trim());
  } catch (_) {
    return null;
  }
  if (!v || v.type !== "session_meta") return null;
  const p = v.payload || {};
  return {
    sessionId: typeof p.id === "string" ? p.id : null,
    cwd: typeof p.cwd === "string" ? p.cwd : null
  };
}
function codexParseGauge(text, marker, isWeekly, nowMs) {
  const re = new RegExp(
    marker + "[\\s\\S]{0,200}?(\\d+(?:\\.\\d+)?)\\s*%\\s*left[\\s\\S]*?\\(resets?\\s+([^)]+?)\\)"
  );
  const caps = re.exec(text);
  if (!caps) return { pct: null, resetsMs: null };
  const left = parseFloat(caps[1]);
  const pct = isNaN(left) ? null : Math.min(100, Math.max(0, 100 - left));
  const resetsRaw = (caps[2] || "").trim();
  const resetsMs = isWeekly ? codexParseWeeklyResets(resetsRaw, nowMs) : codexParseSessionResets(resetsRaw, nowMs);
  return { pct, resetsMs };
}
function codexParseSessionResets(raw, nowMs) {
  const m = /(\d{1,2}):(\d{2})/.exec(String(raw || "").trim());
  if (!m) return null;
  const hh = parseInt(m[1], 10), mm = parseInt(m[2], 10);
  const now = new Date(nowMs);
  let cand = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hh, mm, 0, 0);
  if (cand.getTime() <= nowMs) cand = new Date(cand.getTime() + 24 * 3600 * 1e3);
  return cand.getTime();
}
var CODEX_MONTHS = {
  jan: 0,
  january: 0,
  feb: 1,
  february: 1,
  mar: 2,
  march: 2,
  apr: 3,
  april: 3,
  may: 4,
  jun: 5,
  june: 5,
  jul: 6,
  july: 6,
  aug: 7,
  august: 7,
  sep: 8,
  sept: 8,
  september: 8,
  oct: 9,
  october: 9,
  nov: 10,
  november: 10,
  dec: 11,
  december: 11
};
function isValidCodexSessionDelta(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type !== "response_item") return false;
  if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
  const kind = v.payload.type;
  if (kind !== "message" && kind !== "function_call" && kind !== "function_call_output") return false;
  return true;
}
function validateEntry(v) {
  return isValidCodexSessionDelta(v);
}
function isValidCodexSessionMessage(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type !== "response_item") return false;
  if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
  if (v.payload.type !== "message") return false;
  if (v.payload.role !== "user" && v.payload.role !== "assistant") return false;
  if (!Array.isArray(v.payload.content)) return false;
  return true;
}
function codexParseWeeklyResets(raw, nowMs) {
  const m = /(\d{1,2}):(\d{2})\s+on\s+(\d{1,2})\s+([A-Za-z]+)/.exec(String(raw || "").trim());
  if (!m) return null;
  const hh = parseInt(m[1], 10), mm = parseInt(m[2], 10);
  const day = parseInt(m[3], 10);
  const month = CODEX_MONTHS[m[4].toLowerCase()];
  if (month === void 0) return null;
  const now = new Date(nowMs);
  let cand = new Date(now.getFullYear(), month, day, hh, mm, 0, 0);
  if (cand.getTime() <= nowMs) {
    cand = new Date(now.getFullYear() + 1, month, day, hh, mm, 0, 0);
  }
  return cand.getTime();
}
var plugin_default = plugin;
