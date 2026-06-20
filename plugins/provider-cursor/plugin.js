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

// plugins/provider-cursor/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default
});
module.exports = __toCommonJS(plugin_exports);
var TITLE_RE = /cursor/i;
var OUTPUT_FINGERPRINTS = [
  "Workspace Trust Required",
  "Cursor Agent can execute code",
  "Type ? in the prompt bar",
  "Plan, search, build anything"
];
var TUI_FRAGMENTS = [
  "cursor agent",
  "workspace trust required",
  "type ? in the prompt bar",
  "run everything"
];
var plugin = {
  detectFromTitle(title) {
    return TITLE_RE.test(String(title || ""));
  },
  detectFromOutput(text) {
    const t = String(text || "");
    for (let i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
      if (t.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) return true;
    }
    return t.indexOf("Cursor Agent") !== -1 && t.indexOf("Run Everything") !== -1;
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
      if (c !== "\u2500" && c !== "\u2501" && c !== "\u2550" && c !== "\u2502" && c !== "\u2503" && c !== "\u256D" && c !== "\u256E" && c !== "\u256F" && c !== "\u2570" && c !== " " && c !== "	" && c !== "\n" && c !== "\r") {
        return false;
      }
    }
    return true;
  },
  // Cheap incremental gate: a selector glyph, a keyboard footer, or a
  // trust/approval keyword means a prompt frame might be on screen → let the
  // host read the full screen and run parsePrompt (the authoritative gate).
  hasEventMarkers(chunk) {
    return chunkHasPromptSignal(String(chunk || ""));
  },
  // Surface a waiting state when the full screen parses as an interactive menu.
  detectEvents(text) {
    return parseCursorPrompt(String(text || "")) ? ["permission_request"] : [];
  },
  // Read cursor's alt-screen menu (permission / workspace-trust / model picker)
  // into {question, options}. Returns null on non-menu output so normal chat
  // never surfaces a phantom card. The host navigates via buildPromptResponse.
  parsePrompt(text) {
    return parseCursorPrompt(String(text || ""));
  },
  // Cursor menus are vertical lists navigated with ↑/↓ + Enter. The highlight is
  // NOT the top row — the /model picker lands the → selector on the CURRENT model —
  // so navigate RELATIVE to the live highlight (delta = target − current), not from
  // the top. `screen` is the cleaned terminal text the host passes for this reason;
  // absent it, fall back to top-start. Esc-cancel is the host's separate path.
  // Base64 byte steps: down=ESC[B, up=ESC[A, enter=\r.
  buildPromptResponse(optionIndex, screen) {
    const DOWN = "G1tC";
    const UP = "G1tB";
    const ENTER = "DQ==";
    const target = optionIndex > 0 ? optionIndex : 0;
    const current = screen ? cursorHighlightIndex(String(screen)) : -1;
    const start = current >= 0 ? current : 0;
    const delta = target - start;
    const key = delta >= 0 ? DOWN : UP;
    const steps = [];
    for (let i = 0; i < Math.abs(delta); i++) steps.push(key);
    steps.push(ENTER);
    return steps;
  },
  // Discover via `cursor-agent --list-models` (host-cached); falls back to the manifest seed when offline.
  discoverModels() {
    return discoverCursorModels() || manifestModels();
  },
  // Pre-write cursor's workspace-trust marker to bypass its one-time trust prompt,
  // but only under skip-permissions — that's the user's intentional "trust everything" flow.
  buildLaunchCommand(params) {
    const p = params || {};
    const parts = ["cursor-agent"];
    if (p.skipPermissions) parts.push("--force");
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) {
        parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
      }
    }
    return (p.skipPermissions ? trustPrefix() : "") + parts.join(" ");
  },
  buildResumeCommand(sessionId, skipPermissions) {
    const parts = ["cursor-agent", "--resume", host.shell.quote(String(sessionId || ""))];
    if (skipPermissions) parts.push("--force");
    return (skipPermissions ? trustPrefix() : "") + parts.join(" ");
  },
  // ── Session discovery (sqlite metadata under ~/.cursor/chats/<md5(cwd)>) ──
  sessionExists(cwd, sessionId) {
    const dir = cursorSessionDir(cwd, sessionId);
    if (!dir) return false;
    const sep = pathSep();
    return host.fs.fileExists(dir + sep + "meta.json") || host.fs.fileExists(dir + sep + "store.db");
  },
  // Session birth time (unix ms) from meta.json, for the host's launch-window birth-time guard.
  sessionCreatedMs(cwd, sessionId) {
    const meta = readSessionMeta(cwd, sessionId);
    if (!meta) return null;
    const ms = meta.createdAtMs;
    return typeof ms === "number" ? ms : null;
  },
  // Re-attach a restarted cursor pane to its session. Identity-bound, never
  // "newest-for-cwd" (capability rule #3): with 2+ viable candidates, refuse rather than hand over a sibling's session.
  detectSessionId(cwd, exclude, maxAgeMs) {
    const dir = cursorChatsDir(cwd);
    if (!dir || !host.fs.fileExists(dir)) return null;
    const entries = host.fs.readDir(dir) || [];
    const ex = toExcludeList(exclude);
    const now = host.unixNowMs ? host.unixNowMs() : 0;
    const maxAge = typeof maxAgeMs === "number" && maxAgeMs > 0 ? maxAgeMs : 0;
    const sep = pathSep();
    const candidates = [];
    for (let i = 0; i < entries.length; i++) {
      const e = entries[i];
      if (!e || !e.isDir) continue;
      const sid = e.name;
      if (!looksLikeUuid(sid)) continue;
      if (ex.indexOf(sid) !== -1) continue;
      const meta = readMetaJson(e.path + sep + "meta.json");
      if (!meta && !host.fs.fileExists(e.path + sep + "store.db")) continue;
      const ts = meta ? meta.updatedAtMs || meta.createdAtMs || 0 : e.modifiedMs || 0;
      if (maxAge && now && (!ts || now - ts > maxAge)) continue;
      candidates.push({ id: sid, ts });
    }
    if (candidates.length !== 1) return null;
    return candidates[0].id;
  },
  detectActiveSession(_cwd, _currentSid) {
    return null;
  },
  // Per-turn model is `"modelName"` in store.db JSON blobs (the 0x7B filter skips binary protobuf blobs).
  detectSessionModel(cwd, sessionId) {
    const dir = cursorSessionDir(cwd, sessionId);
    if (!dir) return null;
    const db = dir + pathSep() + "store.db";
    if (!host.fs.fileExists(db)) return null;
    const rows = host.fs.sqliteQuery(
      db,
      "SELECT CAST(data AS TEXT) AS t FROM blobs WHERE substr(data,1,1) = x'7B'",
      []
    );
    let model = null;
    for (let i = 0; i < rows.length; i++) {
      const t = rows[i] && rows[i].t;
      if (typeof t !== "string") continue;
      const m = /"modelName"\s*:\s*"([^"]+)"/.exec(t);
      if (m && m[1]) model = m[1];
    }
    return model;
  },
  enumerateSessions() {
    const root = projectsRoot();
    if (!root || !host.fs.fileExists(root)) return [];
    const files = host.fs.walkDir(root, 5) || [];
    const out = [];
    for (let i = 0; i < files.length; i++) {
      const f = files[i];
      if (!f || !f.isFile || !endsWith(f.name, ".jsonl")) continue;
      if (f.path.indexOf("agent-transcripts") === -1) continue;
      const stem = f.name.substring(0, f.name.length - 6);
      if (!looksLikeUuid(stem)) continue;
      out.push({
        sessionId: stem,
        provider: "cursor",
        projectPath: null,
        title: null,
        summary: null,
        filePath: f.path,
        createdTs: f.createdMs ? Math.floor(f.createdMs / 1e3) : null,
        modifiedTs: f.modifiedMs ? Math.floor(f.modifiedMs / 1e3) : null
      });
    }
    return out;
  },
  // Chat transcript via append-only JSONL tailer: the CLI can't replay a full
  // transcript and store.db is a protobuf tree, so the clean source is cursor's per-session JSONL.
  usesStructuredChat() {
    return false;
  },
  // Live tailer path: the host canonicalizes the cwd first, so the slug matches cursor's own.
  sessionJsonlPath(cwd, sessionId) {
    return cursorTranscriptPath(cwd, sessionId);
  },
  // Snapshot + content search pass a raw cwd, so the scan fallback inside resolves symlinks.
  sessionFilePath(cwd, sessionId) {
    return cursorTranscriptPath(cwd, sessionId);
  },
  // Parse appended JSONL lines into RichMessages. Schema:
  //   {"role":"user"|"assistant","message":{"content":[{type,text|name,input}]}}  ("turn_ended" skipped)
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
          if (typeof console !== "undefined") console.warn("[cursor-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      const role = v && v.role;
      if (role !== "user" && role !== "assistant") continue;
      const content = v.message && v.message.content;
      if (!Array.isArray(content)) continue;
      const blocks = blocksFromContent(content, role);
      if (!blocks.length) continue;
      out.push({ role, blocks, ts: null });
    }
    return { messages: out };
  },
  // Mirror parseSessionDelta for the content-search pipeline (text only).
  parseSessionMessages(text) {
    const out = [];
    const lines = String(text || "").split("\n");
    for (let i = 0; i < lines.length; i++) {
      if (!lines[i]) continue;
      let v;
      try {
        v = JSON.parse(lines[i]);
      } catch (_) {
        continue;
      }
      const role = v && v.role;
      if (role !== "user" && role !== "assistant") continue;
      const content = v.message && v.message.content;
      if (!Array.isArray(content)) continue;
      const txt = textFromContent(content, role);
      if (!txt) continue;
      out.push({ role, text: txt, ts: null, isMeta: false });
    }
    return out;
  },
  // Cursor's interactive TUI has no usage gauge screen, so there's nothing to parse → null.
  parseUsage(_rawText, _nowMs) {
    return null;
  }
};
var CURSOR_UNAVAILABLE_RE = /\s*\((?:unavailable|locked|disabled|coming soon|not available|requires [^)]+|pro only|upgrade required)\)\s*/i;
function discoverCursorModels() {
  const raw = host.exec(JSON.stringify({
    bin: "cursor-agent",
    args: ["--list-models"],
    timeoutMs: 5e3
  }));
  const result = safeJson(raw);
  if (!result || result.code !== 0 || typeof result.stdout !== "string") return null;
  const lines = result.stdout.split("\n");
  const models = [];
  for (let i = 0; i < lines.length; i++) {
    const m = /^([a-z0-9][a-z0-9._-]*)\s+-\s+(.+)$/i.exec(lines[i].trim());
    if (!m) continue;
    const id = m[1];
    if (isNonChatModel(id)) continue;
    const unavailable = CURSOR_UNAVAILABLE_RE.test(m[2]);
    const disp = m[2].replace(CURSOR_UNAVAILABLE_RE, " ").replace(/\s*\((?:current|default)\)\s*$/i, "").trim();
    const info = { id, displayName: disp || id };
    if (unavailable) info.available = false;
    models.push(info);
  }
  return models.length ? models : null;
}
var NON_CHAT_TOKENS = ["embed", "embedding", "embeddings", "tts", "whisper", "moderation", "rerank", "dall", "dalle"];
function isNonChatModel(id) {
  const leaf = (id.split("/").pop() || id).toLowerCase();
  const toks = leaf.split(/[-_.]/);
  for (let i = 0; i < toks.length; i++) if (NON_CHAT_TOKENS.indexOf(toks[i]) !== -1) return true;
  return false;
}
function manifestModels() {
  try {
    const m = JSON.parse(host.manifestJson() || "null");
    return Array.isArray(m && m.models) ? m.models : [];
  } catch (_) {
    return [];
  }
}
function trustPrefix() {
  if (host.platform() === "windows") {
    return [
      "$d=(Get-Location).Path;",
      "$s=($d -replace '[^a-zA-Z0-9]','-');",
      "$p=Join-Path $HOME (Join-Path '.cursor\\projects' $s);",
      "New-Item -ItemType Directory -Force -Path $p | Out-Null;",
      "$tf=Join-Path $p '.workspace-trusted';",
      `if(-not (Test-Path $tf)){('{"trustedAt":"'+(Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.000Z')+'","workspacePath":"'+($d -replace '\\\\','\\\\\\\\')+'"}') | Set-Content -NoNewline $tf};`,
      ""
    ].join(" ");
  }
  return `__ctd="$(pwd -P)"; __cts="$(printf '%s' "\${__ctd#/}" | tr -c 'a-zA-Z0-9' '-')"; __ctp="$HOME/.cursor/projects/$__cts"; __ctdj="$(printf '%s' "$__ctd" | sed 's/\\\\/\\\\\\\\/g; s/"/\\\\"/g')"; mkdir -p "$__ctp" 2>/dev/null; [ -f "$__ctp/.workspace-trusted" ] || printf '{"trustedAt":"%s","workspacePath":"%s"}\\n' "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" "$__ctdj" > "$__ctp/.workspace-trusted" 2>/dev/null; `;
}
function pathSep() {
  return host.platform() === "windows" ? "\\" : "/";
}
function cursorRoot() {
  const home = host.homeDir();
  if (!home) return null;
  return home + pathSep() + ".cursor";
}
function chatsRoot() {
  const r = cursorRoot();
  return r ? r + pathSep() + "chats" : null;
}
function projectsRoot() {
  const r = cursorRoot();
  return r ? r + pathSep() + "projects" : null;
}
function toNativeCwd(cwd) {
  const s = String(cwd || "");
  return host.platform() === "windows" ? s.replace(/\//g, "\\") : s;
}
function cursorChatsDir(cwd) {
  const root = chatsRoot();
  if (!root) return null;
  const hash = host.md5(toNativeCwd(cwd));
  if (!hash) return null;
  return root + pathSep() + hash;
}
function cursorSessionDir(cwd, sessionId) {
  if (!sessionId) return null;
  const dir = cursorChatsDir(cwd);
  return dir ? dir + pathSep() + sessionId : null;
}
function cursorSlug(cwd) {
  return String(cwd || "").replace(/^[/\\]+/, "").replace(/[^a-zA-Z0-9]/g, "-");
}
function cursorTranscriptPath(cwd, sessionId) {
  if (!sessionId) return null;
  const root = projectsRoot();
  if (!root) return null;
  const sep = pathSep();
  const slug = cursorSlug(cwd);
  const tail = sep + "agent-transcripts" + sep + sessionId + sep + sessionId + ".jsonl";
  if (slug) {
    const direct = root + sep + slug + tail;
    if (host.fs.fileExists(direct)) return direct;
  }
  const dirs = host.fs.readDir(root) || [];
  for (let i = 0; i < dirs.length; i++) {
    const d = dirs[i];
    if (!d || !d.isDir) continue;
    const cand = d.path + tail;
    if (host.fs.fileExists(cand)) return cand;
  }
  return slug ? root + sep + slug + tail : null;
}
function readSessionMeta(cwd, sessionId) {
  const dir = cursorSessionDir(cwd, sessionId);
  if (!dir) return null;
  return readMetaJson(dir + pathSep() + "meta.json");
}
function readMetaJson(path) {
  if (!host.fs.fileExists(path)) return null;
  return host.fs.readJson(path);
}
function blocksFromContent(content, role) {
  const blocks = [];
  for (let i = 0; i < content.length; i++) {
    const c = content[i];
    if (!c) continue;
    if (c.type === "text" && typeof c.text === "string") {
      const txt = cleanText(c.text, role);
      if (txt) blocks.push({ kind: "text", data: { text: txt } });
    } else if (c.type === "tool_use") {
      blocks.push({
        kind: "tool_use",
        data: { name: c.name || "tool", input: c.input == null ? {} : c.input }
      });
    }
  }
  return blocks;
}
function textFromContent(content, role) {
  const chunks = [];
  for (let i = 0; i < content.length; i++) {
    const c = content[i];
    if (c && c.type === "text" && typeof c.text === "string") {
      const txt = cleanText(c.text, role);
      if (txt) chunks.push(txt);
    }
  }
  return chunks.join(" ");
}
function cleanText(raw, role) {
  let t = String(raw || "");
  if (role === "user") {
    const m = /^<user_query>\n?([\s\S]*?)\n?<\/user_query>\s*$/.exec(t.trim());
    if (m) t = m[1];
  }
  t = t.split("[REDACTED]").join("");
  t = t.replace(/\n{3,}/g, "\n\n");
  return t.trim();
}
var PROMPT_SELECTORS = "\u276F\u203A\u25B6\u27A4\u2192";
var PROMPT_NUM_RE = /^[❯›▶➤>\s]*?(\d+)[.)]\s+(.+)$/;
function cleanPromptLine(line) {
  return String(line).replace(/[│┃|╎╏┆┊╭╮╯╰]/g, " ").replace(/[─━═]{2,}/g, " ").replace(/\s+/g, " ").trim();
}
function chunkHasPromptSignal(chunk) {
  for (let i = 0; i < PROMPT_SELECTORS.length; i++) {
    if (chunk.indexOf(PROMPT_SELECTORS[i]) !== -1) return true;
  }
  const lc = chunk.toLowerCase();
  return lc.indexOf("workspace trust") !== -1 || lc.indexOf("run everything") !== -1 || lc.indexOf("to continue") !== -1 || lc.indexOf("to cancel") !== -1 || lc.indexOf("to select") !== -1 || // /model list footer "Enter to select"
  lc.indexOf("to filter") !== -1 || // /model list footer "Type to filter"
  lc.indexOf("to navigate") !== -1 || lc.indexOf("to confirm") !== -1 || lc.indexOf("to use") !== -1 || lc.indexOf("approve") !== -1 || lc.indexOf("esc") !== -1 && lc.indexOf("enter") !== -1;
}
function findPromptQuestion(lines, firstIdx) {
  for (let i = firstIdx - 1; i >= 0 && firstIdx - i <= 6; i--) {
    const t = lines[i];
    if (t.length > 3 && t.charAt(t.length - 1) === "?") return t;
  }
  for (let i = firstIdx - 1; i >= 0 && firstIdx - i <= 4; i--) {
    const t = lines[i];
    if (t && t.length > 3) return t;
  }
  return "Permission required";
}
function parseCursorPrompt(text) {
  return parseCursorNumberedMenu(text) || parseCursorListMenu(text);
}
function collectCursorNumberedMenu(text) {
  const lines = String(text || "").split("\n").map(cleanPromptLine);
  if (lines.length < 2) return null;
  const start = Math.max(0, lines.length - 25);
  const collected = [];
  for (let i = start; i < lines.length; i++) {
    const l = lines[i];
    if (!l) continue;
    const m = PROMPT_NUM_RE.exec(l);
    if (m && m[2].trim()) {
      const sel = PROMPT_SELECTORS.indexOf(l.charAt(0)) !== -1 || l.charAt(0) === ">";
      collected.push({ value: m[1], label: m[2].trim(), idx: i, sel });
    }
  }
  if (collected.length < 2) return null;
  const cluster = [];
  let prev = collected[collected.length - 1].idx;
  for (let i = collected.length - 1; i >= 0; i--) {
    if (prev - collected[i].idx <= 2 || cluster.length === 0) {
      cluster.unshift(collected[i]);
      prev = collected[i].idx;
    } else break;
  }
  if (cluster.length < 2) return null;
  const lastIdx = cluster[cluster.length - 1].idx;
  const hasSelector = cluster.some((o) => o.sel);
  let hasFooter = false;
  for (let i = lastIdx; i < Math.min(lines.length, lastIdx + 5); i++) {
    const t = lines[i].toLowerCase();
    if ((t.indexOf("esc") !== -1 || t.indexOf("enter") !== -1) && (t.indexOf("cancel") !== -1 || t.indexOf("confirm") !== -1 || t.indexOf("select") !== -1 || t.indexOf("continue") !== -1)) {
      hasFooter = true;
      break;
    }
  }
  if (!hasSelector && !hasFooter) return null;
  return { question: findPromptQuestion(lines, cluster[0].idx), cluster };
}
function parseCursorNumberedMenu(text) {
  const c = collectCursorNumberedMenu(text);
  if (!c) return null;
  const options = c.cluster.map((o) => [o.label, o.value]);
  return { question: c.question, options };
}
function collectCursorListMenu(text) {
  const raw = String(text || "").split("\n");
  if (raw.length < 3) return null;
  const start = Math.max(0, raw.length - 30);
  let footer = -1;
  for (let i = raw.length - 1; i >= start; i--) {
    const lc = raw[i].toLowerCase();
    if (isCursorListFooter(lc)) {
      footer = i;
      break;
    }
  }
  if (footer === -1) return null;
  const rows = [];
  let question = "Select an option";
  for (let i = footer - 1; i >= start && footer - i <= 18; i--) {
    const line = raw[i];
    const t = line.trim();
    if (!t) continue;
    const lc = t.toLowerCase();
    if (isPaginationLine(t)) continue;
    if (isScrollMarkerLine(t)) continue;
    if (isCursorListFooter(lc)) break;
    if (lc.indexOf("filter:") === 0) continue;
    if (isListQuestionLine(t)) {
      question = firstField(t);
      break;
    }
    const sel = PROMPT_SELECTORS.indexOf(t.charAt(0)) !== -1 || t.charAt(0) === ">";
    if (!sel && !isCursorListOptionLine(line)) {
      if (rows.length) continue;
      question = firstField(t);
      continue;
    }
    const body = sel ? t.substring(1).trim() : t;
    const label = firstField(body);
    if (!label) {
      if (rows.length) break;
      else continue;
    }
    rows.unshift({ label, sel });
  }
  if (rows.length < 1) return null;
  if (rows.length === 1 && !rows[0].sel) return null;
  if (!rows.some((r) => r.sel) && question === "Select an option") return null;
  return { question, rows };
}
function parseCursorListMenu(text) {
  const c = collectCursorListMenu(text);
  if (!c) return null;
  const options = c.rows.map((r, i) => [r.label, String(i + 1)]);
  return { question: c.question, options };
}
function cursorHighlightIndex(text) {
  const n = collectCursorNumberedMenu(text);
  if (n) return n.cluster.findIndex((o) => o.sel);
  const l = collectCursorListMenu(text);
  if (l) return l.rows.findIndex((r) => r.sel);
  return -1;
}
function isPaginationLine(t) {
  return /^\d+\s*-\s*\d+\s+of\s+\d+/i.test(t) || /^\d+\s+of\s+\d+$/i.test(t);
}
function isCursorListFooter(lc) {
  return lc.indexOf("to select") !== -1 || lc.indexOf("to filter") !== -1 || lc.indexOf("to navigate") !== -1 || lc.indexOf("to confirm") !== -1 || lc.indexOf("to use") !== -1;
}
function isScrollMarkerLine(t) {
  return /^[↑↓]\s+\d+\s+more\s+(?:above|below)$/i.test(t);
}
function isListQuestionLine(t) {
  if (t.charAt(t.length - 1) === "?") return true;
  return /^(?:available models|rules \(\d+ rules?\)|skills \(\d+ skills?\))(?:\s|$)/i.test(t);
}
function isCursorListOptionLine(line) {
  const leading = leadingSpaces(line);
  const t = line.trim();
  if (!t) return false;
  if (PROMPT_SELECTORS.indexOf(t.charAt(0)) !== -1 || t.charAt(0) === ">") return true;
  if (leading >= 2 && leading <= 4) return true;
  return false;
}
function leadingSpaces(s) {
  const m = /^ */.exec(s);
  return m ? m[0].length : 0;
}
function firstField(s) {
  const m = /^(.*?)\s{2,}\S/.exec(s);
  const label = (m ? m[1] : s).trim();
  return label.replace(/\s*\(tab to [^)]*\)\s*$/i, "").trim();
}
function toExcludeList(exclude) {
  if (Array.isArray(exclude)) return exclude;
  if (typeof exclude === "string" && exclude) return [exclude];
  return [];
}
function looksLikeUuid(s) {
  return typeof s === "string" && s.length >= 32 && s.indexOf("-") !== -1;
}
function endsWith(s, suffix) {
  return s.length >= suffix.length && s.substring(s.length - suffix.length) === suffix;
}
function safeJson(s) {
  try {
    return JSON.parse(String(s || "null"));
  } catch (_) {
    return null;
  }
}
var plugin_default = plugin;
