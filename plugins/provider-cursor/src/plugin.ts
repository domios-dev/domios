// Cursor (cursor-agent) provider plugin runtime logic, compiled to QuickJS-compatible
// plugin.js by scripts/build-plugin.mjs. Frontend contract lives in src/index.ts.
import type { LaunchParams, ModelInfo, ParsedPrompt, PluginModule } from "@codeterm/plugin-sdk";

const TITLE_RE = /cursor/i;
// Structural fingerprints of the cursor-agent TUI — functional UI strings, not vendor-churned marketing copy.
const OUTPUT_FINGERPRINTS = [
  "Workspace Trust Required",
  "Cursor Agent can execute code",
  "Type ? in the prompt bar",
  "Plan, search, build anything",
];
const TUI_FRAGMENTS = [
  "cursor agent",
  "workspace trust required",
  "type ? in the prompt bar",
  "run everything",
];

const plugin: PluginModule = {
  detectFromTitle(title: string): boolean {
    return TITLE_RE.test(String(title || ""));
  },

  detectFromOutput(text: string): boolean {
    const t = String(text || "");
    for (let i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
      if (t.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) return true;
    }
    // Two-signal match a stray "Cursor Agent" mention in a shell can't reach alone.
    return t.indexOf("Cursor Agent") !== -1 && t.indexOf("Run Everything") !== -1;
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
        c !== "─" && c !== "━" && c !== "═" && c !== "│" && c !== "┃" &&
        c !== "╭" && c !== "╮" && c !== "╯" && c !== "╰" &&
        c !== " " && c !== "\t" && c !== "\n" && c !== "\r"
      ) {
        return false;
      }
    }
    return true;
  },

  // Cheap incremental gate: a selector glyph, a keyboard footer, or a
  // trust/approval keyword means a prompt frame might be on screen → let the
  // host read the full screen and run parsePrompt (the authoritative gate).
  hasEventMarkers(chunk: string): boolean {
    return chunkHasPromptSignal(String(chunk || ""));
  },

  // Surface a waiting state when the full screen parses as an interactive menu.
  detectEvents(text: string): string[] {
    return parseCursorPrompt(String(text || "")) ? ["permission_request"] : [];
  },

  // Read cursor's alt-screen menu (permission / workspace-trust / model picker)
  // into {question, options}. Returns null on non-menu output so normal chat
  // never surfaces a phantom card. The host navigates via buildPromptResponse.
  parsePrompt(text: string): ParsedPrompt | null {
    return parseCursorPrompt(String(text || ""));
  },

  // Cursor menus are vertical lists navigated with ↑/↓ + Enter. The highlight is
  // NOT the top row — the /model picker lands the → selector on the CURRENT model —
  // so navigate RELATIVE to the live highlight (delta = target − current), not from
  // the top. `screen` is the cleaned terminal text the host passes for this reason;
  // absent it, fall back to top-start. Esc-cancel is the host's separate path.
  // Base64 byte steps: down=ESC[B, up=ESC[A, enter=\r.
  buildPromptResponse(optionIndex: number, screen?: string): string[] {
    const DOWN = "G1tC"; // ESC [ B
    const UP = "G1tB"; // ESC [ A
    const ENTER = "DQ=="; // \r
    const target = optionIndex > 0 ? optionIndex : 0;
    const current = screen ? cursorHighlightIndex(String(screen)) : -1;
    const start = current >= 0 ? current : 0;
    const delta = target - start;
    const key = delta >= 0 ? DOWN : UP;
    const steps: string[] = [];
    for (let i = 0; i < Math.abs(delta); i++) steps.push(key);
    steps.push(ENTER);
    return steps;
  },

  // Discover via `cursor-agent --list-models` (host-cached); falls back to the manifest seed when offline.
  discoverModels(): ModelInfo[] {
    return discoverCursorModels() || manifestModels();
  },

  // Pre-write cursor's workspace-trust marker to bypass its one-time trust prompt,
  // but only under skip-permissions — that's the user's intentional "trust everything" flow.
  buildLaunchCommand(params: LaunchParams): string {
    const p = params || {};
    const parts: string[] = ["cursor-agent"];
    if (p.skipPermissions) parts.push("--force");
    if (p.args && p.args.length > 0) {
      // Quote each caller-supplied arg to prevent shell injection.
      for (let i = 0; i < p.args.length; i++) {
        parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
      }
    }
    return (p.skipPermissions ? trustPrefix() : "") + parts.join(" ");
  },

  buildResumeCommand(sessionId: string, skipPermissions?: boolean): string {
    const parts = ["cursor-agent", "--resume", host.shell.quote(String(sessionId || ""))];
    if (skipPermissions) parts.push("--force");
    return (skipPermissions ? trustPrefix() : "") + parts.join(" ");
  },

  // ── Session discovery (sqlite metadata under ~/.cursor/chats/<md5(cwd)>) ──

  sessionExists(cwd: string, sessionId: string): boolean {
    const dir = cursorSessionDir(cwd, sessionId);
    if (!dir) return false;
    const sep = pathSep();
    return host.fs.fileExists(dir + sep + "meta.json") ||
      host.fs.fileExists(dir + sep + "store.db");
  },

  // Session birth time (unix ms) from meta.json, for the host's launch-window birth-time guard.
  sessionCreatedMs(cwd: string, sessionId: string): number | null {
    const meta = readSessionMeta(cwd, sessionId);
    if (!meta) return null;
    const ms = meta.createdAtMs;
    return typeof ms === "number" ? ms : null;
  },

  // Re-attach a restarted cursor pane to its session. Identity-bound, never
  // "newest-for-cwd" (capability rule #3): with 2+ viable candidates, refuse rather than hand over a sibling's session.
  detectSessionId(cwd: string, exclude?: string | string[], maxAgeMs?: number): string | null {
    const dir = cursorChatsDir(cwd);
    if (!dir || !host.fs.fileExists(dir)) return null;
    const entries = host.fs.readDir(dir) || [];
    const ex = toExcludeList(exclude);
    const now = host.unixNowMs ? host.unixNowMs() : 0;
    const maxAge = typeof maxAgeMs === "number" && maxAgeMs > 0 ? maxAgeMs : 0;
    const sep = pathSep();
    const candidates: { id: string; ts: number }[] = [];
    for (let i = 0; i < entries.length; i++) {
      const e = entries[i];
      if (!e || !e.isDir) continue;
      const sid = e.name;
      if (!looksLikeUuid(sid)) continue;
      if (ex.indexOf(sid) !== -1) continue;
      // Only real sessions (same existence contract as sessionExists); ignore stale/empty temp dirs.
      const meta = readMetaJson(e.path + sep + "meta.json");
      if (!meta && !host.fs.fileExists(e.path + sep + "store.db")) continue;
      // Current cursor builds write only store.db (no meta.json), so meta
      // timestamps are absent. Fall back to the session dir's mtime — without a
      // timestamp every session has ts=0, the maxAge window can't filter, and 2+
      // sessions in a reused cwd make detection refuse (chat never binds). With
      // mtime, only the just-spawned session falls inside maxAge.
      const ts = meta ? (meta.updatedAtMs || meta.createdAtMs || 0) : (e.modifiedMs || 0);
      if (maxAge && now && (!ts || (now - ts) > maxAge)) continue;
      candidates.push({ id: sid, ts: ts });
    }
    if (candidates.length !== 1) return null;
    return candidates[0].id;
  },

  detectActiveSession(_cwd: string, _currentSid?: string): string | null {
    return null;
  },

  // Per-turn model is `"modelName"` in store.db JSON blobs (the 0x7B filter skips binary protobuf blobs).
  detectSessionModel(cwd: string, sessionId: string): string | null {
    const dir = cursorSessionDir(cwd, sessionId);
    if (!dir) return null;
    const db = dir + pathSep() + "store.db";
    if (!host.fs.fileExists(db)) return null;
    const rows = host.fs.sqliteQuery<{ t?: string }>(
      db,
      "SELECT CAST(data AS TEXT) AS t FROM blobs WHERE substr(data,1,1) = x'7B'",
      []
    );
    let model: string | null = null;
    for (let i = 0; i < rows.length; i++) {
      const t = rows[i] && rows[i].t;
      if (typeof t !== "string") continue;
      const m = /"modelName"\s*:\s*"([^"]+)"/.exec(t);
      if (m && m[1]) model = m[1];
    }
    return model;
  },

  enumerateSessions(): unknown[] {
    const root = projectsRoot();
    if (!root || !host.fs.fileExists(root)) return [];
    const files = host.fs.walkDir(root, 5) || [];
    const out: unknown[] = [];
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
        createdTs: f.createdMs ? Math.floor(f.createdMs / 1000) : null,
        modifiedTs: f.modifiedMs ? Math.floor(f.modifiedMs / 1000) : null,
      });
    }
    return out;
  },

  // Chat transcript via append-only JSONL tailer: the CLI can't replay a full
  // transcript and store.db is a protobuf tree, so the clean source is cursor's per-session JSONL.

  usesStructuredChat(): boolean {
    return false;
  },

  // Live tailer path: the host canonicalizes the cwd first, so the slug matches cursor's own.
  sessionJsonlPath(cwd: string, sessionId: string): string | null {
    return cursorTranscriptPath(cwd, sessionId);
  },

  // Snapshot + content search pass a raw cwd, so the scan fallback inside resolves symlinks.
  sessionFilePath(cwd: string, sessionId: string): string | null {
    return cursorTranscriptPath(cwd, sessionId);
  },

  // Parse appended JSONL lines into RichMessages. Schema:
  //   {"role":"user"|"assistant","message":{"content":[{type,text|name,input}]}}  ("turn_ended" skipped)
  parseSessionDelta(chunk: string): unknown {
    const out: unknown[] = [];
    const lines = String(chunk || "").split("\n");
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
      if (!lines[i]) continue;
      let v: CursorEntry;
      try { v = JSON.parse(lines[i]); } catch (_) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[cursor-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      const role = v && v.role;
      if (role !== "user" && role !== "assistant") continue; // skip turn_ended etc.
      const content = v.message && v.message.content;
      if (!Array.isArray(content)) continue;
      const blocks = blocksFromContent(content, role);
      if (!blocks.length) continue;
      out.push({ role: role, blocks: blocks, ts: null });
    }
    return { messages: out };
  },

  // Mirror parseSessionDelta for the content-search pipeline (text only).
  parseSessionMessages(text: string): unknown[] {
    const out: unknown[] = [];
    const lines = String(text || "").split("\n");
    for (let i = 0; i < lines.length; i++) {
      if (!lines[i]) continue;
      let v: CursorEntry;
      try { v = JSON.parse(lines[i]); } catch (_) { continue; }
      const role = v && v.role;
      if (role !== "user" && role !== "assistant") continue;
      const content = v.message && v.message.content;
      if (!Array.isArray(content)) continue;
      const txt = textFromContent(content, role);
      if (!txt) continue;
      out.push({ role: role, text: txt, ts: null, isMeta: false });
    }
    return out;
  },

  // Cursor's interactive TUI has no usage gauge screen, so there's nothing to parse → null.
  parseUsage(_rawText: string, _nowMs: number): unknown {
    return null;
  },
};

// ── types ──

interface CursorManifest { models?: ModelInfo[] }
interface CursorContentBlock { type?: string; text?: string; name?: string; input?: unknown }
interface CursorMessage { role?: string; content?: CursorContentBlock[] }
interface CursorEntry { role?: string; type?: string; message?: CursorMessage }
interface CursorSessionMeta {
  schemaVersion?: number;
  createdAtMs?: number;
  updatedAtMs?: number;
  hasConversation?: boolean;
}

// ── model discovery ──

// Annotations cursor appends that mean the account/plan cannot select the model.
const CURSOR_UNAVAILABLE_RE = /\s*\((?:unavailable|locked|disabled|coming soon|not available|requires [^)]+|pro only|upgrade required)\)\s*/i;

function discoverCursorModels(): ModelInfo[] | null {
  const raw = host.exec(JSON.stringify({
    bin: "cursor-agent",
    args: ["--list-models"],
    timeoutMs: 5000,
  }));
  const result = safeJson(raw) as { code?: number; stdout?: string } | null;
  if (!result || result.code !== 0 || typeof result.stdout !== "string") return null;
  const lines = result.stdout.split("\n");
  const models: ModelInfo[] = [];
  for (let i = 0; i < lines.length; i++) {
    const m = /^([a-z0-9][a-z0-9._-]*)\s+-\s+(.+)$/i.exec(lines[i].trim());
    if (!m) continue;
    const id = m[1];
    if (isNonChatModel(id)) continue;
    const unavailable = CURSOR_UNAVAILABLE_RE.test(m[2]);
    // Strip status/lockout annotations ("(current)", "(default)", "(locked)"); keep
    // informative ones like "(NO ZDR)".
    const disp = m[2]
      .replace(CURSOR_UNAVAILABLE_RE, " ")
      .replace(/\s*\((?:current|default)\)\s*$/i, "")
      .trim();
    const info: ModelInfo = { id: id, displayName: disp || id };
    if (unavailable) info.available = false; // greyed in the picker, like claude's fable
    models.push(info);
  }
  return models.length ? models : null;
}

// Duplicated across provider bundles by design (QuickJS, no imports) — drops
// embedding/tts/moderation/rerank ids so only chat models reach the picker.
const NON_CHAT_TOKENS = ["embed", "embedding", "embeddings", "tts", "whisper", "moderation", "rerank", "dall", "dalle"];
function isNonChatModel(id: string): boolean {
  const leaf = (id.split("/").pop() || id).toLowerCase();
  const toks = leaf.split(/[-_.]/);
  for (let i = 0; i < toks.length; i++) if (NON_CHAT_TOKENS.indexOf(toks[i]) !== -1) return true;
  return false;
}

function manifestModels(): ModelInfo[] {
  try {
    const m = JSON.parse(host.manifestJson() || "null") as CursorManifest | null;
    return Array.isArray(m && m.models) ? m!.models! : [];
  } catch (_) {
    return [];
  }
}

// ── workspace trust ──

// Shell snippet that writes cursor's `.workspace-trusted` marker for the launch
// cwd (resolved + slugged exactly like cursor) unless it already exists.
function trustPrefix(): string {
  if (host.platform() === "windows") {
    return [
      "$d=(Get-Location).Path;",
      "$s=($d -replace '[^a-zA-Z0-9]','-');",
      "$p=Join-Path $HOME (Join-Path '.cursor\\projects' $s);",
      "New-Item -ItemType Directory -Force -Path $p | Out-Null;",
      "$tf=Join-Path $p '.workspace-trusted';",
      "if(-not (Test-Path $tf)){('{\"trustedAt\":\"'+(Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.000Z')+'\",\"workspacePath\":\"'+($d -replace '\\\\','\\\\\\\\')+'\"}') | Set-Content -NoNewline $tf};",
      "",
    ].join(" ");
  }
  // POSIX: pwd -P matches cursor's realpath; sed JSON-escapes the path so odd chars don't break the trust file.
  return (
    "__ctd=\"$(pwd -P)\"; " +
    "__cts=\"$(printf '%s' \"${__ctd#/}\" | tr -c 'a-zA-Z0-9' '-')\"; " +
    "__ctp=\"$HOME/.cursor/projects/$__cts\"; " +
    "__ctdj=\"$(printf '%s' \"$__ctd\" | sed 's/\\\\/\\\\\\\\/g; s/\"/\\\\\"/g')\"; " +
    "mkdir -p \"$__ctp\" 2>/dev/null; " +
    "[ -f \"$__ctp/.workspace-trusted\" ] || " +
    "printf '{\"trustedAt\":\"%s\",\"workspacePath\":\"%s\"}\\n' " +
    "\"$(date -u +%Y-%m-%dT%H:%M:%S.000Z)\" \"$__ctdj\" > \"$__ctp/.workspace-trusted\" 2>/dev/null; "
  );
}

// ── path helpers ──

function pathSep(): string {
  return host.platform() === "windows" ? "\\" : "/";
}

function cursorRoot(): string | null {
  const home = host.homeDir();
  if (!home) return null;
  return home + pathSep() + ".cursor";
}

function chatsRoot(): string | null {
  const r = cursorRoot();
  return r ? r + pathSep() + "chats" : null;
}

function projectsRoot(): string | null {
  const r = cursorRoot();
  return r ? r + pathSep() + "projects" : null;
}

// Cursor hashes the cwd in its OWN native separator (backslash on Windows). The host hands
// plugins forward-slash paths, so on Windows we must restore backslashes before md5 — else the
// hash misses cursor's dir and the session never binds (chat stays empty). No-op on Unix.
function toNativeCwd(cwd: string): string {
  const s = String(cwd || "");
  return host.platform() === "windows" ? s.replace(/\//g, "\\") : s;
}

// ~/.cursor/chats/<md5(native-cwd)> — cursor keys the chats dir by md5 of its native-separator cwd.
function cursorChatsDir(cwd: string): string | null {
  const root = chatsRoot();
  if (!root) return null;
  const hash = host.md5(toNativeCwd(cwd));
  if (!hash) return null;
  return root + pathSep() + hash;
}

function cursorSessionDir(cwd: string, sessionId: string): string | null {
  if (!sessionId) return null;
  const dir = cursorChatsDir(cwd);
  return dir ? dir + pathSep() + sessionId : null;
}

// cursor's project slug: leading separators stripped, every non-alphanumeric → '-'.
function cursorSlug(cwd: string): string {
  return String(cwd || "").replace(/^[/\\]+/, "").replace(/[^a-zA-Z0-9]/g, "-");
}

// ~/.cursor/projects/<slug>/agent-transcripts/<uuid>/<uuid>.jsonl. Prefer the slug
// path, else scan project dirs for the unique uuid; returns the slug path even when
// absent so the tailer can watch for it to appear.
function cursorTranscriptPath(cwd: string, sessionId: string): string | null {
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

function readSessionMeta(cwd: string, sessionId: string): CursorSessionMeta | null {
  const dir = cursorSessionDir(cwd, sessionId);
  if (!dir) return null;
  return readMetaJson(dir + pathSep() + "meta.json");
}

function readMetaJson(path: string): CursorSessionMeta | null {
  if (!host.fs.fileExists(path)) return null;
  return host.fs.readJson<CursorSessionMeta>(path);
}

// ── content mapping ──

function blocksFromContent(content: CursorContentBlock[], role: string): unknown[] {
  const blocks: unknown[] = [];
  for (let i = 0; i < content.length; i++) {
    const c = content[i];
    if (!c) continue;
    if (c.type === "text" && typeof c.text === "string") {
      const txt = cleanText(c.text, role);
      if (txt) blocks.push({ kind: "text", data: { text: txt } });
    } else if (c.type === "tool_use") {
      blocks.push({
        kind: "tool_use",
        data: { name: c.name || "tool", input: c.input == null ? {} : c.input },
      });
    }
  }
  return blocks;
}

function textFromContent(content: CursorContentBlock[], role: string): string {
  const chunks: string[] = [];
  for (let i = 0; i < content.length; i++) {
    const c = content[i];
    if (c && c.type === "text" && typeof c.text === "string") {
      const txt = cleanText(c.text, role);
      if (txt) chunks.push(txt);
    }
  }
  return chunks.join(" ");
}

// Strip cursor's `<user_query>` framing and `[REDACTED]` markers, then collapse the blank-line runs left behind.
function cleanText(raw: string, role: string): string {
  let t = String(raw || "");
  if (role === "user") {
    const m = /^<user_query>\n?([\s\S]*?)\n?<\/user_query>\s*$/.exec(t.trim());
    if (m) t = m[1];
  }
  t = t.split("[REDACTED]").join("");
  t = t.replace(/\n{3,}/g, "\n\n");
  return t.trim();
}

// ── interactive prompt parsing ──

const PROMPT_SELECTORS = "❯›▶➤→";
// Selector-or-numbered option line, optional leading selector glyph: "❯ 1. Yes" / "2) No".
const PROMPT_NUM_RE = /^[❯›▶➤>\s]*?(\d+)[.)]\s+(.+)$/;

// Strip box-drawing chrome so labels/question stay clean regardless of framing.
function cleanPromptLine(line: string): string {
  return String(line)
    .replace(/[│┃|╎╏┆┊╭╮╯╰]/g, " ")
    .replace(/[─━═]{2,}/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

// Quick chunk-level signal that a prompt frame may be present (gates the
// expensive full-screen parse). Liberal by design — parseCursorPrompt is the
// real gate, so a false hit only costs one cheap parse that returns null.
function chunkHasPromptSignal(chunk: string): boolean {
  for (let i = 0; i < PROMPT_SELECTORS.length; i++) {
    if (chunk.indexOf(PROMPT_SELECTORS[i]) !== -1) return true;
  }
  const lc = chunk.toLowerCase();
  return (
    lc.indexOf("workspace trust") !== -1 ||
    lc.indexOf("run everything") !== -1 ||
    lc.indexOf("to continue") !== -1 ||
    lc.indexOf("to cancel") !== -1 ||
    lc.indexOf("to select") !== -1 || // /model list footer "Enter to select"
    lc.indexOf("to filter") !== -1 || // /model list footer "Type to filter"
    lc.indexOf("approve") !== -1 ||
    (lc.indexOf("esc") !== -1 && lc.indexOf("enter") !== -1)
  );
}

function findPromptQuestion(lines: string[], firstIdx: number): string {
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

// Numbered menus (trust gate, command approval) parse via the host's universal
// shape; the /model picker is a non-numbered list cursor draws instead → try that
// fallback so both layouts surface a card.
function parseCursorPrompt(text: string): ParsedPrompt | null {
  return parseCursorNumberedMenu(text) || parseCursorListMenu(text);
}

// Parse the active vertical menu near the bottom of cursor's screen. Mirrors the
// host's universal numbered-picker shape (so numbered menus never regress) and
// requires a selector glyph or keyboard footer near the options so a plain
// numbered list in prose is not mistaken for a prompt.
interface NumberedCluster { question: string; cluster: { value: string; label: string; idx: number; sel: boolean }[] }
function collectCursorNumberedMenu(text: string): NumberedCluster | null {
  const lines = String(text || "").split("\n").map(cleanPromptLine);
  if (lines.length < 2) return null;
  const start = Math.max(0, lines.length - 25);

  const collected: { value: string; label: string; idx: number; sel: boolean }[] = [];
  for (let i = start; i < lines.length; i++) {
    const l = lines[i];
    if (!l) continue;
    const m = PROMPT_NUM_RE.exec(l);
    if (m && m[2].trim()) {
      const sel = PROMPT_SELECTORS.indexOf(l.charAt(0)) !== -1 || l.charAt(0) === ">";
      collected.push({ value: m[1], label: m[2].trim(), idx: i, sel: sel });
    }
  }
  if (collected.length < 2) return null;

  // Last contiguous cluster — the active menu sits at the bottom.
  const cluster: typeof collected = [];
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
    if (
      (t.indexOf("esc") !== -1 || t.indexOf("enter") !== -1) &&
      (t.indexOf("cancel") !== -1 || t.indexOf("confirm") !== -1 ||
        t.indexOf("select") !== -1 || t.indexOf("continue") !== -1)
    ) { hasFooter = true; break; }
  }
  if (!hasSelector && !hasFooter) return null;

  return { question: findPromptQuestion(lines, cluster[0].idx), cluster: cluster };
}

function parseCursorNumberedMenu(text: string): ParsedPrompt | null {
  const c = collectCursorNumberedMenu(text);
  if (!c) return null;
  const options: [string, string][] = c.cluster.map((o) => [o.label, o.value]);
  return { question: c.question, options: options };
}

// Cursor's /model picker is a NON-numbered vertical list: a selector glyph (→)
// marks the highlight, labels carry trailing metadata after a 2-space gap, and a
// "Type to filter • Enter to select" footer gates it. The host's numbered reader
// can't see it. Works on RAW lines (no whitespace collapse) so the 2-space gap that
// separates a label from its metadata survives. Footer-gated, so prose never cards.
interface ListMenu { question: string; rows: { label: string; sel: boolean }[] }
function collectCursorListMenu(text: string): ListMenu | null {
  const raw = String(text || "").split("\n");
  if (raw.length < 3) return null;
  const start = Math.max(0, raw.length - 30);

  // Anchor on the LAST picker footer (bottom-most = the active menu).
  let footer = -1;
  for (let i = raw.length - 1; i >= start; i--) {
    const lc = raw[i].toLowerCase();
    if (lc.indexOf("to select") !== -1 || lc.indexOf("to filter") !== -1) { footer = i; break; }
  }
  if (footer === -1) return null;

  const rows: { label: string; sel: boolean }[] = [];
  let question = "Select an option";
  for (let i = footer - 1; i >= start && footer - i <= 18; i--) {
    const t = raw[i].trim();
    if (!t) continue; // cursor pads blank lines between the title and the rows
    const lc = t.toLowerCase();
    if (isPaginationLine(t)) continue;
    if (lc.indexOf("to select") !== -1 || lc.indexOf("to filter") !== -1) break; // previous cluster's footer
    if (lc.indexOf("filter:") === 0) continue; // the filter input row, not an option
    if (t.charAt(t.length - 1) === "?") { question = firstField(t); break; }
    if (lc.indexOf("available models") === 0) { question = firstField(t); break; } // picker title
    const sel = PROMPT_SELECTORS.indexOf(t.charAt(0)) !== -1 || t.charAt(0) === ">";
    const body = sel ? t.substring(1).trim() : t;
    const label = firstField(body);
    if (!label) { if (rows.length) break; else continue; }
    rows.unshift({ label: label, sel: sel });
  }
  if (rows.length < 2) return null;
  if (!rows.some((r) => r.sel) && question === "Select an option") return null; // need a highlight or a title
  return { question: question, rows: rows };
}

function parseCursorListMenu(text: string): ParsedPrompt | null {
  const c = collectCursorListMenu(text);
  if (!c) return null;
  const options: [string, string][] = c.rows.map((r, i) => [r.label, String(i + 1)]);
  return { question: c.question, options: options };
}

// 0-based screen-order index of the highlighted (→) row in the active menu, or
// -1 when none is detectable. Mirrors parseCursorPrompt's numbered-then-list
// precedence so the index lines up with the option ordering the card received —
// buildPromptResponse navigates relative to it instead of assuming a top start.
function cursorHighlightIndex(text: string): number {
  const n = collectCursorNumberedMenu(text);
  if (n) return n.cluster.findIndex((o) => o.sel);
  const l = collectCursorListMenu(text);
  if (l) return l.rows.findIndex((r) => r.sel);
  return -1;
}

// "1-10 of 28" / "3 of 28" pagination row — never an option.
function isPaginationLine(t: string): boolean {
  return /^\d+\s*-\s*\d+\s+of\s+\d+/i.test(t) || /^\d+\s+of\s+\d+$/i.test(t);
}

// The label is the text before a 2+ space gap (drops trailing metadata like
// "300K High" or a "(Tab to modify)" hint); single-spaced multi-word names stay whole.
function firstField(s: string): string {
  const m = /^(.*?)\s{2,}\S/.exec(s);
  const label = (m ? m[1] : s).trim();
  return label.replace(/\s*\(tab to [^)]*\)\s*$/i, "").trim();
}

// ── misc helpers ──

function toExcludeList(exclude?: string | string[]): string[] {
  if (Array.isArray(exclude)) return exclude;
  if (typeof exclude === "string" && exclude) return [exclude];
  return [];
}

function looksLikeUuid(s: string): boolean {
  return typeof s === "string" && s.length >= 32 && s.indexOf("-") !== -1;
}

function endsWith(s: string, suffix: string): boolean {
  return s.length >= suffix.length && s.substring(s.length - suffix.length) === suffix;
}

function safeJson(s: string | undefined): unknown {
  try { return JSON.parse(String(s || "null")); } catch (_) { return null; }
}

export default plugin;
