// Cursor (cursor-agent) provider plugin (runtime logic, capability: agentProvider).
// Authored in TypeScript against @codeterm/plugin-sdk and compiled to a
// QuickJS-compatible plugin.js by scripts/build-plugin.mjs. Full integration:
// detect fingerprints, launch/resume (with workspace-trust pre-write), dynamic
// model discovery, sqlite-backed session metadata, and JSONL chat transcripts.
// The frontend ProviderPlugin contract lives in the separate src/index.ts.
import type { LaunchParams, ModelInfo, PluginModule } from "@codeterm/plugin-sdk";

const TITLE_RE = /cursor/i;
// Structural fingerprints of the cursor-agent TUI (rendered screen), captured
// from a live session. These are functional UI strings (trust gate, banner
// hint, force-mode status label), not marketing copy that vendors churn.
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
    // Banner app-name plus the force-mode status label is a two-signal match
    // that a stray "Cursor Agent" mention in a shell can't reach alone.
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

  hasEventMarkers(_chunk: string): boolean {
    return false;
  },

  detectEvents(_text: string): string[] {
    return [];
  },

  // Dynamic model discovery via `cursor-agent --list-models`; host caches the
  // result so this sync probe stays off hot reads. Falls back to the curated
  // manifest seed when the CLI is missing/offline.
  discoverModels(): ModelInfo[] {
    return discoverCursorModels() || manifestModels();
  },

  // The cursor-agent TUI gates every cwd behind a one-time "Workspace Trust
  // Required" prompt. Pre-writing cursor's own trust marker
  // (`~/.cursor/projects/<slug>/.workspace-trusted`) from the launch shell
  // silently bypasses that gate — so only do it when the user opted into
  // skip-permissions (`--force`); that is their intentional "trust everything"
  // flow. Without it, let cursor show its own trust prompt.
  buildLaunchCommand(params: LaunchParams): string {
    const p = params || {};
    const parts: string[] = ["cursor-agent"];
    if (p.skipPermissions) parts.push("--force");
    if (p.args && p.args.length > 0) {
      // Quote each arg — caller-supplied via the spawn API; prevents shell injection.
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

  // Session birth time (unix ms) from meta.json, so the host's launch-window
  // birth-time guard works on cursor's sqlite-backed sessions.
  sessionCreatedMs(cwd: string, sessionId: string): number | null {
    const meta = readSessionMeta(cwd, sessionId);
    if (!meta) return null;
    const ms = meta.createdAtMs;
    return typeof ms === "number" ? ms : null;
  },

  // Re-attach a restarted cursor pane to its session dir under the cwd-hash dir.
  // Identity-bound, never "newest-for-cwd" (capability rule #3): when 2+ viable
  // candidates remain after sibling exclusions, refuse — return null instead of
  // handing the pane a SIBLING's session. The exclude list collapses the set as
  // siblings get bound, so the last unclaimed session still resolves cleanly.
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
      const meta = readMetaJson(e.path + sep + "meta.json");
      const ts = meta ? (meta.updatedAtMs || meta.createdAtMs || 0) : 0;
      if (maxAge && now && ts && (now - ts) > maxAge) continue;
      candidates.push({ id: sid, ts: ts });
    }
    if (candidates.length !== 1) return null;
    return candidates[0].id;
  },

  detectActiveSession(_cwd: string, _currentSid?: string): string | null {
    return null;
  },

  // Per-turn model lives as `"modelName"` inside the JSON message blobs in
  // store.db (binary protobuf tree blobs are skipped via the 0x7B first-byte
  // filter). Returns the raw cursor model id; the host resolves it to a label.
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

  // ── Chat transcript: append-only JSONL tailer (NOT structured-chat). The
  // CLI cannot replay a full transcript (`--resume -p` emits only the new turn)
  // and store.db is a protobuf merkle tree, so the clean source is the
  // per-session JSONL cursor writes under ~/.cursor/projects/<slug>/agent-transcripts. ──

  usesStructuredChat(): boolean {
    return false;
  },

  // Live tailer path: host canonicalizes the cwd (manifest.sessions cwdEncoding)
  // before this call, so the slug matches cursor's own.
  sessionJsonlPath(cwd: string, sessionId: string): string | null {
    return cursorTranscriptPath(cwd, sessionId);
  },

  // Pane-less snapshot + content search read this with a raw (uncanonicalized)
  // cwd, so the scan fallback inside resolves symlinked paths too.
  sessionFilePath(cwd: string, sessionId: string): string | null {
    return cursorTranscriptPath(cwd, sessionId);
  },

  // Parse appended JSONL lines into RichMessages. Cursor transcript schema:
  //   {"role":"user"|"assistant","message":{"content":[{type,text|name,input}]}}
  //   {"type":"turn_ended",...}  (metadata, skipped)
  // `<user_query>…</user_query>` framing and redacted-thinking `[REDACTED]`
  // markers are stripped; tool_use blocks map to tool_use blocks.
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

  // Cursor's interactive TUI has no usage gauge screen (per-turn tokens surface
  // only in the `--print` stream-json `result.usage`, which the PTY never
  // shows), so there's nothing to parse from the rendered screen → null.
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
    // Strip status annotations cursor appends ("(current)", "(default)").
    const disp = m[2].replace(/\s*\((?:current|default)\)\s*$/i, "").trim();
    models.push({ id: id, displayName: disp || id });
  }
  return models.length ? models : null;
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

// Single-line shell snippet that writes cursor's `.workspace-trusted` marker for
// the launch cwd (resolved + slugged exactly like cursor) unless it already
// exists, then a separator so the cursor-agent invocation follows.
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
  // POSIX: pwd -P resolves symlinks (matches cursor's realpath); tr maps every
  // non-alphanumeric to '-' after the leading slash is stripped.
  return (
    "__ctd=\"$(pwd -P)\"; " +
    "__cts=\"$(printf '%s' \"${__ctd#/}\" | tr -c 'a-zA-Z0-9' '-')\"; " +
    "__ctp=\"$HOME/.cursor/projects/$__cts\"; " +
    "mkdir -p \"$__ctp\" 2>/dev/null; " +
    "[ -f \"$__ctp/.workspace-trusted\" ] || " +
    "printf '{\"trustedAt\":\"%s\",\"workspacePath\":\"%s\"}\\n' " +
    "\"$(date -u +%Y-%m-%dT%H:%M:%S.000Z)\" \"$__ctd\" > \"$__ctp/.workspace-trusted\" 2>/dev/null; "
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

// ~/.cursor/chats/<md5(cwd)> — cursor keys the chats dir by md5 of the canonical
// cwd. Real workspaces are already canonical; only symlinked probe dirs differ.
function cursorChatsDir(cwd: string): string | null {
  const root = chatsRoot();
  if (!root) return null;
  const hash = host.md5(String(cwd || ""));
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

// ~/.cursor/projects/<slug>/agent-transcripts/<uuid>/<uuid>.jsonl. Prefer the
// slug-derived path; if it doesn't exist (raw vs canonical cwd), scan project
// dirs for the globally-unique uuid. Returns the slug path even when absent so
// the tailer can watch for it to appear.
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

// Strip cursor's `<user_query>` framing (user turns) and redacted-thinking
// `[REDACTED]` markers, then collapse the blank-line runs they leave behind.
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
