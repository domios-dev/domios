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
  starterPromptText(prompt) {
    if (prompt === "no_starter") return void 0;
    if (prompt === "idle_wakeup") return "You are a newly started agent. Wait for instructions from the user or your orchestration parent.";
    if (prompt === "team_bootstrap") return void 0;
    return "Execute the assigned task.";
  },
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
    if (lower.indexOf("workspace trust required") !== -1) return false;
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
  hasEventMarkers(chunk) {
    return chunkHasPromptSignal(String(chunk || ""));
  },
  // Surface a waiting state when the full screen parses as an interactive menu.
  detectEvents(text) {
    return parseCursorPrompt(String(text || "")) ? ["permission_request"] : [];
  },
  parsePrompt(text) {
    return parseCursorPrompt(String(text || ""));
  },
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
    const startupHandshake = p.starterPrompt === "startup_handshake" ? String(p.starterPromptText || "").trim() : "";
    const launchTask = [startupHandshake, p.task].filter(Boolean).join("\n\n");
    if (launchTask) {
      parts.push(host.shell.quoteFor(launchTask, host.platform()));
    }
    return disableClaudeHookImportPrefix() + (p.skipPermissions ? trustPrefix() : "") + parts.join(" ");
  },
  buildResumeCommand(sessionId, skipPermissions) {
    const parts = [
      "cursor-agent",
      "--resume",
      host.shell.quoteFor(String(sessionId || ""), host.platform())
    ];
    if (skipPermissions) parts.push("--force");
    return disableClaudeHookImportPrefix() + (skipPermissions ? trustPrefix() : "") + parts.join(" ");
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
      out.push(cursorTranscriptRecord(root, stem, f));
    }
    return out;
  },
  // Probes the transcript layout `sessionFilePath` resolves, once per project dir; never walks history.
  findSession(sessionId) {
    if (!looksLikeUuid(sessionId) || !/^[A-Za-z0-9_-]+$/.test(sessionId)) return null;
    const root = projectsRoot();
    if (!root || !host.fs.fileExists(root)) return null;
    const sep = pathSep();
    const transcriptName = sessionId + ".jsonl";
    const dirs = host.fs.readDir(root) || [];
    for (let i = 0; i < dirs.length; i++) {
      const d = dirs[i];
      if (!d || !d.isDir) continue;
      const dir = d.path + sep + "agent-transcripts" + sep + sessionId;
      if (!host.fs.fileExists(dir + sep + transcriptName)) continue;
      const entries = host.fs.readDir(dir) || [];
      for (let j = 0; j < entries.length; j++) {
        const f = entries[j];
        if (f && f.isFile && f.name === transcriptName) return cursorTranscriptRecord(root, sessionId, f);
      }
    }
    return null;
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
    let stamped = 0;
    const stampRows = (lineIndex) => {
      for (; stamped < out.length; stamped++) {
        out[stamped].recordIndex = lineIndex;
      }
    };
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
      stampRows(i - 1);
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
    stampRows(lines.length - 1);
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
  // GetCurrentPeriodUsage carries everything: ready percentages + spend (cents) + billing cycle.
  fetchUsage(nowMs) {
    const token = readCursorToken();
    if (!token) return null;
    const current = cursorApi("GetCurrentPeriodUsage", {}, token);
    if (!current) return null;
    const planUsage = typeof current.planUsage === "object" && current.planUsage ? current.planUsage : null;
    if (!planUsage) return null;
    const spendCents = numFrom(planUsage.totalSpend);
    if (spendCents == null) return null;
    const limitCents = numFrom(planUsage.limit);
    const includedSpendCents = numFrom(planUsage.includedSpend);
    const remainingCents = limitCents != null && includedSpendCents != null ? Math.max(0, limitCents - includedSpendCents) : null;
    return JSON.stringify({
      totalPct: numFrom(planUsage.totalPercentUsed),
      apiPct: numFrom(planUsage.apiPercentUsed),
      autoPct: numFrom(planUsage.autoPercentUsed),
      spendCents,
      limitCents,
      remainingCents,
      resetsAtMs: numFrom(current.billingCycleEnd),
      periodStartMs: numFrom(current.billingCycleStart),
      fetchedAtMs: nowMs
    });
  },
  // Cursor's API returns ready percentages — map verbatim, never re-derive.
  parseUsage(rawText, nowMs) {
    const data = safeJson(String(rawText || ""));
    if (!data || typeof data.spendCents !== "number") return null;
    const resetsAt = typeof data.resetsAtMs === "number" ? data.resetsAtMs : null;
    return {
      provider: "cursor",
      account_id: null,
      captured_at_ms: nowMs,
      session_pct: typeof data.apiPct === "number" ? data.apiPct : null,
      session_resets_at_ms: resetsAt,
      weekly_pct: typeof data.totalPct === "number" ? data.totalPct : null,
      weekly_resets_at_ms: resetsAt,
      spend_cents: data.spendCents,
      spend_limit_cents: typeof data.limitCents === "number" ? data.limitCents : null,
      spend_remaining_cents: typeof data.remainingCents === "number" ? data.remainingCents : null,
      spend_resets_at_ms: resetsAt,
      raw_text: String(rawText || "")
    };
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
function disableClaudeHookImportPrefix() {
  return host.shell.envPrefix("CLAUDE_CODE_DISABLE_HOOKS", "1", host.platform());
}
function trustPrefix() {
  if (host.platform() === "windows") {
    return [
      "$d=(Get-Location).Path;",
      "$s=($d -replace '[^a-zA-Z0-9]+','-');",
      "$p=Join-Path $HOME (Join-Path '.cursor\\projects' $s);",
      "New-Item -ItemType Directory -Force -Path $p | Out-Null;",
      "$tf=Join-Path $p '.workspace-trusted';",
      "$j=@{trustedAt=(Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.000Z');workspacePath=$d}|ConvertTo-Json -Compress;",
      "[IO.File]::WriteAllText($tf,$j);",
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
function projectPathFromTranscript(root, transcript) {
  const sep = pathSep();
  const prefix = root + sep;
  if (transcript.indexOf(prefix) !== 0) return null;
  const slug = transcript.substring(prefix.length).split(sep)[0] || "";
  let candidate;
  if (host.platform() === "windows" && /^[a-zA-Z]-/.test(slug)) {
    candidate = slug.charAt(0).toUpperCase() + ":\\" + slug.substring(2).replace(/-/g, "\\");
  } else {
    candidate = "/" + slug.replace(/-/g, "/");
  }
  return host.fs.fileExists(candidate) ? candidate : null;
}
function cursorTranscriptRecord(root, sessionId, transcript) {
  return {
    sessionId,
    provider: "cursor",
    projectPath: projectPathFromTranscript(root, transcript.path),
    title: null,
    summary: null,
    filePath: transcript.path,
    createdTs: transcript.createdMs ? Math.floor(transcript.createdMs / 1e3) : null,
    modifiedTs: transcript.modifiedMs ? Math.floor(transcript.modifiedMs / 1e3) : null
  };
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
      const canonical = canonicalToolFields(c.name || "", c.input);
      blocks.push({
        kind: "tool_use",
        data: {
          name: c.name || "tool",
          input: c.input == null ? {} : c.input,
          tool_kind: canonical.kind,
          tool_summary: canonical.summary,
          tool_detail: canonical.detail,
          tool_edits: canonical.edits,
          tool_status: "success"
        }
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
function canonicalToolFields(name, input) {
  const tool = name.trim();
  const value = input && typeof input === "object" && !Array.isArray(input) ? input : {};
  const text = (key) => typeof value[key] === "string" ? value[key] : "";
  const command = text("command");
  const path = text("file_path") || text("filePath") || text("path");
  const pattern = text("pattern");
  const edits = extractCanonicalEdits(value);
  const lower = tool.toLowerCase();
  if (lower === "bash" || lower === "shell" || lower === "exec") {
    return { kind: "command", summary: cleanToolText(command, "Command"), detail: command, edits: [] };
  }
  if (lower === "edit" || lower === "multiedit" || lower === "write") {
    return { kind: "edit", summary: path || "File edit", detail: path, edits };
  }
  if (lower === "read") {
    return { kind: "read", summary: path || "Read file", detail: path, edits: [] };
  }
  if (lower === "glob" || lower === "grep" || lower === "search") {
    return { kind: "search", summary: cleanToolText(pattern || path, "Search"), detail: pattern || path, edits: [] };
  }
  return { kind: "generic", summary: cleanToolText(tool, "Tool"), detail: tool, edits };
}
function extractCanonicalEdits(input) {
  const parentPath = typeof input.file_path === "string" ? input.file_path : typeof input.filePath === "string" ? input.filePath : typeof input.path === "string" ? input.path : "";
  const pair = (value, fallbackPath) => {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const item = value;
    const oldValue = typeof item.old_string === "string" ? item.old_string : item.oldString;
    const newValue = typeof item.new_string === "string" ? item.new_string : item.newString;
    if (typeof oldValue !== "string" && typeof newValue !== "string") return null;
    const path = typeof item.file_path === "string" ? item.file_path : typeof item.filePath === "string" ? item.filePath : typeof item.path === "string" ? item.path : fallbackPath;
    return { path, old: typeof oldValue === "string" ? oldValue : "", new: typeof newValue === "string" ? newValue : "" };
  };
  if (Array.isArray(input.edits)) return input.edits.map((edit) => pair(edit, parentPath)).filter((e) => e !== null);
  const single = pair(input, parentPath);
  return single ? [single] : [];
}
function cleanToolText(value, fallback) {
  const text = String(value || "").replace(/[\r\n\t]+/g, " ").trim();
  if (!text) return fallback;
  return text.length > 120 ? text.slice(0, 117) + "..." : text;
}
function cleanText(raw, role) {
  let t = String(raw || "");
  if (role === "user") {
    const m = /^(?:<timestamp>[\s\S]*?<\/timestamp>\s*)?<user_query>\s*([\s\S]*?)\s*<\/user_query>\s*$/.exec(t.trim());
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
var CURSOR_API_BASE = "https://api2.cursor.sh/aiserver.v1.DashboardService/";
function readCursorToken() {
  const platform = host.platform();
  let opts = null;
  if (platform === "macos") {
    opts = {
      bin: "security",
      args: ["find-generic-password", "-s", "cursor-access-token", "-a", "cursor-user", "-w"],
      timeoutMs: 4e3
    };
  } else if (platform === "linux") {
    opts = {
      bin: "secret-tool",
      args: ["lookup", "service", "cursor-access-token", "account", "cursor-user"],
      timeoutMs: 4e3
    };
  }
  if (!opts) return null;
  const r = safeJson(host.exec(JSON.stringify(opts)));
  if (r && r.code === 0 && typeof r.stdout === "string") {
    const tok = r.stdout.trim();
    if (tok) return tok;
  }
  return null;
}
function cursorApi(method, body, token) {
  const raw = host.fetch(
    JSON.stringify({
      url: CURSOR_API_BASE + method,
      method: "POST",
      timeoutMs: 6e3,
      headers: {
        Authorization: "Bearer " + token,
        "Content-Type": "application/json",
        "connect-protocol-version": "1"
      },
      body: JSON.stringify(body)
    })
  );
  const resp = safeJson(raw);
  if (!resp || resp.status !== 200 || typeof resp.body !== "string") return null;
  const parsed = safeJson(resp.body);
  return parsed && typeof parsed === "object" ? parsed : null;
}
function numFrom(v) {
  if (typeof v === "number" && isFinite(v)) return v;
  if (typeof v === "string" && v.trim()) {
    const n = Number(v);
    if (isFinite(n)) return n;
  }
  return null;
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
